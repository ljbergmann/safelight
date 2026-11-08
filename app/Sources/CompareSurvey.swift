import SwiftUI

/// The photos Compare or Survey is open for.
struct ChoosingRequest: Identifiable {
    enum Mode { case compare, survey }
    let id = UUID()
    let mode: Mode
    let ids: [Int64]
}

/// A photo's preview, as large as the view asks, from its embedded JPEG.
struct LargePreview: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var image: UIImage?
    @State private var failed = false

    var body: some View {
        ZStack {
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
            } else if failed {
                Image(systemName: "photo").font(.largeTitle).foregroundStyle(Theme.secondaryText)
            } else {
                ProgressView()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: photo.id) {
            failed = false
            image = await model.thumbnail(photo, edge: 1600)
            failed = image == nil
        }
    }
}

/// A photo's flag and stars, for choosing between photos.
struct ChoiceBar: View {
    let photo: LibraryPhoto
    let model: LibraryModel

    var body: some View {
        HStack(spacing: 12) {
            Button { model.setFlag(photo, to: photo.flag == 1 ? 0 : 1) } label: {
                Image(systemName: photo.flag == 1 ? "flag.fill" : "flag")
            }
            Button { model.setFlag(photo, to: photo.flag == -1 ? 0 : -1) } label: {
                Image(systemName: photo.flag == -1 ? "xmark.circle.fill" : "xmark.circle")
                    .foregroundStyle(photo.flag == -1 ? Theme.accent : Theme.secondaryText)
            }
            Divider().frame(height: 18)
            ForEach(1...5, id: \.self) { n in
                Button { model.setRating(photo, to: photo.rating == n ? 0 : Int32(n)) } label: {
                    Image(systemName: n <= photo.rating ? "star.fill" : "star")
                        .foregroundStyle(n <= photo.rating ? .yellow : Theme.secondaryText)
                }
            }
        }
        .buttonStyle(.plain)
        .font(.callout)
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(Theme.raised.opacity(0.9), in: Capsule())
    }
}

/// Lightroom's Survey: the chosen photos side by side, as large as the
/// screen lets them, each with its flag and stars; × takes one out of the
/// survey (not the library).
struct SurveyView: View {
    let model: LibraryModel
    @State var ids: [Int64]
    @Environment(\.dismiss) private var dismiss

    private var shown: [LibraryPhoto] { ids.compactMap { id in model.photos.first { $0.id == id } } }

    var body: some View {
        NavigationStack {
            GeometryReader { geometry in
                let columns = Self.columns(for: shown.count, in: geometry.size)
                let rows = Int((Double(shown.count) / Double(columns)).rounded(.up))
                let height = (geometry.size.height - CGFloat(rows + 1) * 8) / CGFloat(max(rows, 1))
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: columns), spacing: 8) {
                    ForEach(shown, id: \.id) { photo in
                        LargePreview(photo: photo, model: model)
                            .frame(height: height)
                            .overlay(alignment: .bottom) { ChoiceBar(photo: photo, model: model).padding(8) }
                            .overlay(alignment: .topTrailing) {
                                Button { ids.removeAll { $0 == photo.id } } label: {
                                    Image(systemName: "xmark.circle.fill").font(.title2)
                                        .foregroundStyle(.white, .black.opacity(0.6))
                                }
                                .padding(8)
                                .accessibilityLabel("Take out of the survey")
                            }
                    }
                }
                .padding(8)
            }
            .background(Theme.canvas)
            .navigationTitle("Survey · \(shown.count) photos")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }

    /// Columns that leave the photos largest, for `count` photos of about
    /// 3:2 in `size`.
    static func columns(for count: Int, in size: CGSize) -> Int {
        guard count > 1 else { return 1 }
        var best = (columns: 1, edge: 0.0)
        for c in 1...count {
            let rows = Int((Double(count) / Double(c)).rounded(.up))
            let cell = CGSize(width: size.width / CGFloat(c), height: size.height / CGFloat(rows))
            let edge = min(cell.width, cell.height * 1.5)
            if edge > best.edge { best = (c, edge) }
        }
        return best.columns
    }
}

/// Lightroom's Compare: the Select on the left, a Candidate on the right,
/// the arrows stepping the Candidate through the other chosen photos;
/// Swap trades them, Make Select keeps the Candidate.
struct CompareView: View {
    let model: LibraryModel
    let ids: [Int64]
    @State private var select = 0
    @State private var candidate = 1
    @Environment(\.dismiss) private var dismiss

    private func photo(_ i: Int) -> LibraryPhoto? {
        guard ids.indices.contains(i) else { return nil }
        return model.photos.first { $0.id == ids[i] }
    }

    var body: some View {
        NavigationStack {
            HStack(spacing: 8) {
                side("Select", photo(select))
                side("Candidate", photo(candidate))
            }
            .padding(8)
            .background(Theme.canvas)
            .navigationTitle("Compare")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItemGroup(placement: .bottomBar) {
                    Button("Previous Candidate", systemImage: "arrow.left") { step(-1) }
                    Button("Next Candidate", systemImage: "arrow.right") { step(1) }
                    Spacer()
                    Button("Swap", systemImage: "arrow.left.arrow.right") { swap(&select, &candidate) }
                    Button("Make Select") { select = candidate; step(1) }
                }
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }

    private func side(_ title: String, _ photo: LibraryPhoto?) -> some View {
        VStack(spacing: 8) {
            Text(title).font(.headline).foregroundStyle(Theme.secondaryText)
            if let photo {
                LargePreview(photo: photo, model: model)
                Text(photo.title).font(.caption).foregroundStyle(Theme.secondaryText)
                ChoiceBar(photo: photo, model: model)
            } else {
                Spacer()
            }
        }
        .frame(maxWidth: .infinity)
    }

    /// The next Candidate that isn't the Select, round the chosen photos.
    private func step(_ by: Int) {
        guard ids.count > 1 else { return }
        var next = candidate
        repeat { next = (next + by + ids.count) % ids.count } while next == select
        candidate = next
    }
}

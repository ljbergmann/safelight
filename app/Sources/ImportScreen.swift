import SwiftUI

/// What the import screen shows of a card or folder: its photos grouped by
/// the day they will be filed under, which of them are new, and the choice.
@MainActor @Observable
final class ImportChoice {
    let folder: URL
    private(set) var candidates: [ImportCandidate] = []
    private(set) var scanning = true
    private(set) var error: String?
    /// Paths of the photos to import.
    var chosen: Set<String> = []

    init(folder: URL) { self.folder = folder }

    /// The photos by day, oldest first, as the import will file them.
    var days: [(day: String, photos: [ImportCandidate])] {
        var order: [String] = []
        var byDay: [String: [ImportCandidate]] = [:]
        for c in candidates {
            if byDay[c.day] == nil { order.append(c.day) }
            byDay[c.day, default: []].append(c)
        }
        return order.sorted().map { ($0, byDay[$0]!) }
    }

    var new: [ImportCandidate] { candidates.filter { !$0.duplicate } }
    var chosenBytes: UInt64 { candidates.filter { chosen.contains($0.path) }.reduce(0) { $0 + $1.size } }

    /// Lists the folder; every new photo starts chosen.
    func scan(in library: Library) async {
        let path = folder.path
        let outcome = await Task.detached { Result { try library.scanForImport(folder: path) } }.value
        scanning = false
        switch outcome {
        case .success(let found):
            candidates = found
            chosen = Set(found.filter { !$0.duplicate }.map(\.path))
        case .failure(let error):
            self.error = "\(error)"
        }
    }

    func toggle(_ c: ImportCandidate) {
        guard !c.duplicate else { return }
        if chosen.contains(c.path) { chosen.remove(c.path) } else { chosen.insert(c.path) }
    }

    /// Chooses a day's new photos, or lets them go when all are chosen.
    func toggleDay(_ photos: [ImportCandidate]) {
        let new = photos.filter { !$0.duplicate }.map(\.path)
        if new.allSatisfy(chosen.contains) { chosen.subtract(new) } else { chosen.formUnion(new) }
    }

    func chooseAllNew() { chosen = Set(new.map(\.path)) }
    func chooseNone() { chosen = [] }

    /// The chosen photos, in the order listed.
    var chosenPaths: [String] { candidates.map(\.path).filter(chosen.contains) }
}

/// Importing from a card or a folder: its photos as a grid by day, those the
/// catalog already has marked and left out, the new ones chosen to start
/// with. Import copies the chosen ones into the catalog's dated folders and
/// leaves the card as it was.
struct ImportScreen: View {
    let model: LibraryModel
    @State var choice: ImportChoice
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Group {
                if choice.scanning {
                    ProgressView("Looking for photos…")
                } else if let error = choice.error {
                    ContentUnavailableView("Couldn’t read this location", systemImage: "exclamationmark.triangle", description: Text(error))
                } else if choice.candidates.isEmpty {
                    ContentUnavailableView("No photos here", systemImage: "photo.on.rectangle",
                                           description: Text("No RAW or image files were found in this location."))
                } else {
                    grid
                }
            }
            .navigationTitle(choice.folder.lastPathComponent)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Import \(choice.chosen.count)") {
                        let paths = choice.chosenPaths
                        let folder = choice.folder
                        dismiss()
                        Task { await model.importFromFolder(folder, paths: paths) }
                    }
                    .disabled(choice.chosen.isEmpty)
                }
                ToolbarItemGroup(placement: .bottomBar) {
                    Button("All New") { choice.chooseAllNew() }.disabled(choice.new.isEmpty)
                    Button("None") { choice.chooseNone() }.disabled(choice.chosen.isEmpty)
                    Spacer()
                    Text(summary).font(.callout).foregroundStyle(.secondary)
                }
            }
        }
        .task {
            guard let library = model.library else { return }
            await choice.scan(in: library)
        }
    }

    private var summary: String {
        let already = choice.candidates.count - choice.new.count
        let size = ByteCountFormatter.string(fromByteCount: Int64(choice.chosenBytes), countStyle: .file)
        var parts = ["\(choice.chosen.count) of \(choice.new.count) new chosen (\(size))"]
        if already > 0 { parts.append("\(already) already in the catalog") }
        return parts.joined(separator: " · ")
    }

    private var grid: some View {
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 8)], spacing: 8, pinnedViews: .sectionHeaders) {
                ForEach(choice.days, id: \.day) { day, photos in
                    SwiftUI.Section {
                        ForEach(photos, id: \.path) { c in
                            ImportCell(candidate: c, chosen: choice.chosen.contains(c.path), model: model)
                                .onTapGesture { choice.toggle(c) }
                        }
                    } header: {
                        dayHeader(day, photos)
                    }
                }
            }
            .padding()
        }
    }

    private func dayHeader(_ day: String, _ photos: [ImportCandidate]) -> some View {
        let new = photos.filter { !$0.duplicate }
        let all = !new.isEmpty && new.allSatisfy { choice.chosen.contains($0.path) }
        return HStack {
            Text(Self.title(of: day)).font(.headline)
            Text("\(photos.count) photo\(photos.count == 1 ? "" : "s")").foregroundStyle(.secondary)
            Spacer()
            if !new.isEmpty {
                Button(all ? "Deselect Day" : "Select Day") { choice.toggleDay(photos) }.font(.callout)
            }
        }
        .padding(.vertical, 6)
        .background(.bar)
    }

    /// "Monday, 22 April 2023" for "2023-04-22"; the day as it is otherwise.
    static func title(of day: String) -> String {
        let parse = DateFormatter()
        parse.locale = Locale(identifier: "en_US_POSIX")
        parse.timeZone = TimeZone(identifier: "UTC")
        parse.dateFormat = "yyyy-MM-dd"
        guard let date = parse.date(from: day) else { return day }
        var style = Date.FormatStyle(date: .complete, time: .omitted)
        style.timeZone = TimeZone(identifier: "UTC")!
        return date.formatted(style)
    }
}

/// A photo of the card: its thumbnail, chosen or not; one the catalog has is
/// dimmed and can't be chosen.
struct ImportCell: View {
    let candidate: ImportCandidate
    let chosen: Bool
    let model: LibraryModel
    @State private var image: UIImage?

    var body: some View {
        Color(white: 0.15)
            .aspectRatio(1, contentMode: .fit)
            .overlay {
                if let image { Image(uiImage: image).resizable().scaledToFit() } else { ProgressView() }
            }
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .opacity(candidate.duplicate ? 0.4 : 1)
            .overlay(alignment: .topTrailing) {
                if candidate.duplicate {
                    Text("In catalog").font(.caption2.bold()).padding(.horizontal, 5).padding(.vertical, 2)
                        .background(.black.opacity(0.6), in: Capsule()).foregroundStyle(.white).padding(5)
                } else {
                    Image(systemName: chosen ? "checkmark.circle.fill" : "circle")
                        .font(.title3).foregroundStyle(.white, Color.accentColor).padding(5)
                }
            }
            .overlay(alignment: .bottomLeading) {
                Text(candidate.filename).font(.caption2).lineLimit(1).padding(4)
                    .background(.black.opacity(0.45)).foregroundStyle(.white)
            }
            .contentShape(Rectangle())
            .task { image = await model.thumbnail(path: candidate.path, edge: 256) }
            .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

extension LibraryModel {
    /// Copies `paths`, chosen from `folder` on the import screen, into the
    /// catalog, keeping the folder's security scope open until it is done.
    func importFromFolder(_ folder: URL, paths: [String]) async {
        let scoped = folder.startAccessingSecurityScopedResource()
        defer { if scoped { folder.stopAccessingSecurityScopedResource() } }
        setImporting("Importing \(paths.count) photos…")
        let notes = await runImport(paths, mode: .copy)
        finishImport("From \(folder.lastPathComponent): " + notes.joined(separator: " · "))
    }

    /// `-import-screen-test`: lists the catalog's own photo folder as the
    /// import screen lists a card, where every photo must show as already in
    /// the catalog and none be chosen. Prints `IMPORT SCREEN …`.
    func importScreenTest() async {
        guard let library else { return }
        let choice = ImportChoice(folder: URL(fileURLWithPath: library.photosFolder()))
        let start = ContinuousClock.now
        await choice.scan(in: library)
        let ms = (ContinuousClock.now - start).components
        let thumb = await thumbnail(path: choice.candidates.first?.path ?? "", edge: 256)
        print("IMPORT SCREEN \(choice.candidates.count) photos in \(choice.days.count) days (first \(choice.days.first.map { ImportScreen.title(of: $0.day) } ?? "-")), \(choice.new.count) new, \(choice.chosen.count) chosen; listed in \(ms.seconds * 1000 + ms.attoseconds / 1_000_000_000_000_000) ms; thumbnail \(thumb.map { "\(Int($0.size.width))×\(Int($0.size.height))" } ?? "none"); error \(choice.error ?? "none")")
    }
}

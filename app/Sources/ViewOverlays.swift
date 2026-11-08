import SwiftUI

/// Lightroom's Info Overlay: the photo's name, when it was taken, and its
/// camera, lens and settings, over the photo's corner.
struct PhotoInfoOverlay: View {
    let library: Library
    let photo: LibraryPhoto
    @State private var camera: [String] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(photo.isCopy ? "\(photo.filename) · \(photo.copyName)" : photo.filename)
                .font(.callout.weight(.semibold))
            if !photo.captured.isEmpty { Text(Self.readable(photo.captured)) }
            ForEach(camera, id: \.self) { Text($0) }
        }
        .font(.caption)
        .foregroundStyle(.white)
        .shadow(color: .black.opacity(0.8), radius: 2)
        .padding(8)
        .background(.black.opacity(0.35), in: RoundedRectangle(cornerRadius: 8))
        .allowsHitTesting(false)
        .task(id: photo.id) {
            let (library, id) = (library, photo.id)
            camera = await Task.detached { (try? library.metadata(id: id))?.camera ?? [] }.value
        }
    }

    /// "2026-03-24T17:07:09.000" as "24 Mar 2026 at 17:07:09", in the
    /// user's own date style; as stored when it doesn't read as a date.
    private static func readable(_ captured: String) -> String {
        let parse = DateFormatter()
        parse.locale = Locale(identifier: "en_US_POSIX")
        let trimmed = String(captured.replacingOccurrences(of: "T", with: " ").prefix(19))
        parse.dateFormat = "yyyy-MM-dd HH:mm:ss"
        guard let date = parse.date(from: trimmed) else { return captured }
        return date.formatted(date: .abbreviated, time: .standard)
    }
}

/// Reference View's other side: the reference photo as edited, beside the
/// one being edited, as Lightroom shows it.
struct ReferencePane: View {
    let library: Library
    let photo: LibraryPhoto
    let model: LiveModel
    @State private var image: UIImage?
    @State private var failed: String?

    var body: some View {
        ZStack(alignment: .topLeading) {
            Theme.canvas
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let failed {
                Text(failed).font(.callout).foregroundStyle(Theme.secondaryText).padding()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            HStack(spacing: 6) {
                Text("Reference").font(.caption.bold())
                Text(photo.filename).font(.caption)
                Button {
                    model.referenceView = false
                } label: {
                    Image(systemName: "xmark").font(.caption.bold())
                }
                .accessibilityLabel("Close Reference View")
            }
            .padding(.horizontal, 8).padding(.vertical, 4)
            .background(.black.opacity(0.55), in: Capsule()).foregroundStyle(.white)
            .padding(8)
        }
        .overlay(alignment: .trailing) { Rectangle().fill(.black).frame(width: 2) }
        .task(id: photo.id) {
            image = nil
            failed = nil
            let (library, id) = (library, photo.id)
            let made = await Task.detached { Result { try library.referenceImage(id: id, edge: 2048) } }.value
            switch made {
            case .success(let path): image = UIImage(contentsOfFile: path)
            case .failure(RenderError.Failed(let message)): failed = message
            case .failure(let error): failed = "\(error)"
            }
        }
    }
}

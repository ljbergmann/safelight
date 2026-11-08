import SwiftUI
import UniformTypeIdentifiers

/// The catalog's photos as a grid; tap one to edit it.
struct LibraryView: View {
    @State private var model = LibraryModel()
    @State private var path: [LibraryPhoto.ID] = []
    @State private var picking = false

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 12)], spacing: 12) {
                    ForEach(model.photos, id: \.id) { photo in
                        NavigationLink(value: photo.id) {
                            ThumbnailCell(photo: photo, model: model)
                        }
                        .buttonStyle(.plain)
                        .contextMenu { RatingMenu(photo: photo, model: model) }
                    }
                }
                .padding()
            }
            .overlay {
                if model.photos.isEmpty && model.importing == nil {
                    ContentUnavailableView {
                        Label("No photos yet", systemImage: "photo.on.rectangle")
                    } description: {
                        Text("Import RAW files from the Files app. They're copied into RAWmakase's catalog.")
                    } actions: {
                        Button("Import Photos…") { picking = true }.buttonStyle(.borderedProminent)
                    }
                }
            }
            .navigationTitle("Library")
            .toolbar {
                ToolbarItem(placement: .status) {
                    if let importing = model.importing {
                        HStack { ProgressView(); Text(importing) }
                    } else {
                        Text("\(model.photos.count) photos").foregroundStyle(.secondary).fixedSize()
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Import", systemImage: "square.and.arrow.down") { picking = true }
                        .disabled(model.library == nil || model.importing != nil)
                }
            }
            .fileImporter(
                isPresented: $picking,
                allowedContentTypes: [.rawImage, .image],
                allowsMultipleSelection: true
            ) { result in
                if case .success(let files) = result { Task { await model.importFiles(files) } }
            }
            .alert("Something went wrong", isPresented: .constant(model.error != nil)) {
                Button("OK") {}
            } message: {
                Text(model.error ?? "")
            }
            .navigationDestination(for: LibraryPhoto.ID.self) { id in
                if let photo = model.photos.first(where: { $0.id == id }) {
                    EditorView(photo: photo, model: model)
                }
            }
        }
        .task {
            await model.open()
            let args = ProcessInfo.processInfo.arguments
            if args.contains("-import-documents") { await model.importDocuments() }
            model.rateFromArguments()
            // -open <file name>: start in the editor, for -sweep and -pan.
            if let i = args.firstIndex(of: "-open"), i + 1 < args.count,
               let photo = model.photos.first(where: { $0.filename == args[i + 1] }) {
                path = [photo.id]
            }
            if args.contains("-benchmark") { Diagnostics.benchmark() }
            if args.contains("-allocprobe") { Memory.probeAllocator() }
            if args.contains("-thumbnails") { await measureThumbnails() }
        }
    }

    /// Makes every thumbnail, four at a time, and prints how long it took:
    /// the `-thumbnails` launch option.
    private func measureThumbnails() async {
        let start = ContinuousClock.now
        await withTaskGroup(of: Void.self) { group in
            for photo in model.photos { group.addTask { _ = await model.thumbnail(photo, edge: 340) } }
        }
        let memory = Memory.footprint()
        print("THUMBNAILS \(model.photos.count) in \(start.duration(to: .now)); memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak))")
    }
}

extension LibraryPhoto: Identifiable {}

struct ThumbnailCell: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var image: UIImage?
    @State private var loaded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Color(white: 0.15)
                .aspectRatio(1, contentMode: .fit)
                .overlay {
                    if let image {
                        Image(uiImage: image).resizable().scaledToFit()
                    } else if loaded {
                        Image(systemName: "photo").font(.largeTitle).foregroundStyle(.secondary)
                    } else {
                        ProgressView()
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 6))
            HStack {
                Text(photo.filename).font(.caption).lineLimit(1)
                Spacer()
                Stars(rating: photo.rating).font(.caption2)
            }
        }
        .task(id: photo.id) {
            image = await model.thumbnail(photo, edge: 340)
            loaded = true
        }
    }
}

struct Stars: View {
    let rating: Int32
    var body: some View {
        HStack(spacing: 1) {
            ForEach(1...5, id: \.self) { n in
                Image(systemName: n <= rating ? "star.fill" : "star")
                    .foregroundStyle(n <= rating ? .yellow : .secondary)
            }
        }
    }
}

struct RatingMenu: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    var body: some View {
        ForEach(0...5, id: \.self) { n in
            Button(n == 0 ? "No rating" : String(repeating: "★", count: n)) {
                model.setRating(photo, to: Int32(n))
            }
        }
    }
}

/// The live editor for one photo of the catalog, with its rating.
struct EditorView: View {
    let photo: LibraryPhoto
    let model: LibraryModel

    private var rating: Int32 { model.photos.first { $0.id == photo.id }?.rating ?? photo.rating }

    var body: some View {
        LiveEditor(photo: URL(fileURLWithPath: photo.path))
            .navigationTitle(photo.filename)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    HStack(spacing: 2) {
                        ForEach(1...5, id: \.self) { n in
                            Button {
                                model.setRating(photo, to: rating == n ? 0 : Int32(n))
                            } label: {
                                Image(systemName: n <= rating ? "star.fill" : "star")
                                    .foregroundStyle(n <= rating ? .yellow : .secondary)
                            }
                        }
                    }
                }
            }
    }
}

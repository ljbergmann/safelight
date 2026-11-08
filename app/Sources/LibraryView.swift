import SwiftUI
import UniformTypeIdentifiers

/// The catalog's photos as a grid; tap one to edit it.
struct LibraryView: View {
    @State private var model = LibraryModel()
    @State private var path: [LibraryPhoto.ID] = []
    @State private var picking = false
    /// Select mode: taps choose photos instead of opening them.
    @State private var selecting = false
    @State private var selected: Set<Int64> = []
    @State private var pasteNote: String?
    @State private var exporting = false

    var body: some View {
        NavigationStack(path: $path) {
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 12)], spacing: 12) {
                    ForEach(model.photos, id: \.id) { photo in
                        if selecting {
                            Button {
                                if selected.contains(photo.id) { selected.remove(photo.id) } else { selected.insert(photo.id) }
                            } label: {
                                ThumbnailCell(photo: photo, model: model)
                                    .overlay(alignment: .topTrailing) {
                                        Image(systemName: selected.contains(photo.id) ? "checkmark.circle.fill" : "circle")
                                            .font(.title2).foregroundStyle(.white, Color.accentColor).padding(6)
                                    }
                            }
                            .buttonStyle(.plain)
                        } else {
                            NavigationLink(value: photo.id) {
                                ThumbnailCell(photo: photo, model: model)
                            }
                            .buttonStyle(.plain)
                            .contextMenu { RatingMenu(photo: photo, model: model) }
                        }
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
                    } else if let pasteNote {
                        Text(pasteNote).foregroundStyle(.secondary).lineLimit(1)
                    } else {
                        Text("\(model.photos.count) photos").foregroundStyle(.secondary).fixedSize()
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Import", systemImage: "square.and.arrow.down") { picking = true }
                        .disabled(model.library == nil || model.importing != nil)
                }
                ToolbarItem(placement: .primaryAction) {
                    Button(selecting ? "Done" : "Select") {
                        selecting.toggle()
                        selected = []
                    }
                }
                if selecting {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Export", systemImage: "square.and.arrow.up") { exporting = true }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button("Paste Settings", systemImage: "doc.on.clipboard") {
                            let ids = Array(selected)
                            let notes = (try? model.library?.pasteSettings(ids: ids)) ?? []
                            pasteNote = notes.isEmpty ? "Pasted onto \(ids.count) photos" : notes.joined(separator: " · ")
                            selecting = false
                            selected = []
                        }
                        .disabled(selected.isEmpty || !(model.library?.hasCopiedSettings() ?? false))
                    }
                }
            }
            .sheet(isPresented: $exporting) {
                if let library = model.library {
                    ExportSheet(library: library, ids: model.photos.map(\.id).filter(selected.contains))
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
            // -export <file name>: a full-size JPEG of it, timed, for measuring.
            if let i = args.firstIndex(of: "-export"), i + 1 < args.count,
               let photo = model.photos.first(where: { $0.filename == args[i + 1] }), let library = model.library {
                let start = ContinuousClock.now
                let folder = URL.temporaryDirectory.appending(path: "Export-test").path
                let outcome = await Task.detached {
                    Result { try library.exportPhotos(ids: [photo.id], choice: ExportChoice(format: .jpeg, quality: 90, longEdge: 0), folder: folder) }
                }.value
                let memory = Memory.footprint()
                switch outcome {
                case .success(let files):
                    let size = (try? FileManager.default.attributesOfItem(atPath: files[0])[.size] as? Int) ?? 0
                    print("EXPORT \(files[0].split(separator: "/").last ?? "") \(size / 1024) KB in \(start.duration(to: .now)); memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak))")
                case .failure(let e): print("EXPORT FAILED \(e)")
                }
            }
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

/// The live editor for one photo of the catalog, with its rating. Leaving
/// saves the edit first; if that fails, the editor stays and says why.
struct EditorView: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var live = LiveModel()
    @State private var saveFailed = false
    @Environment(\.dismiss) private var dismiss

    private var rating: Int32 { model.photos.first { $0.id == photo.id }?.rating ?? photo.rating }

    var body: some View {
        Group {
            if let library = model.library {
                LiveEditor(photo: photo, library: library, model: live)
            }
        }
        .navigationTitle(photo.filename)
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden()
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button("Library", systemImage: "chevron.backward") {
                    if live.save() { dismiss() } else { saveFailed = true }
                }
            }
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
        // Leaving the app may be the last chance: iPadOS can end it in the
        // background without asking. Resigning active comes first (the app
        // switcher, a call), then the background; both save. The scene
        // phase isn't passed on to this view inside the navigation stack.
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willResignActiveNotification)) { _ in
            live.save()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.didEnterBackgroundNotification)) { _ in
            live.save()
        }
        .alert("The edit couldn't be saved", isPresented: $saveFailed) {
            Button("Try Again") { if live.save() { dismiss() } else { saveFailed = true } }
            Button("Stay", role: .cancel) {}
        } message: {
            Text(live.edit?.saveError ?? "")
        }
    }
}

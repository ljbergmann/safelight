import SwiftUI
import UniformTypeIdentifiers

/// The catalog's photos as a grid; tap one to edit it.
struct LibraryView: View {
    @State private var model = LibraryModel()
    @State private var path: [LibraryPhoto.ID] = []
    @State private var picking = false
    @State private var pickingPhotos = false
    /// What the Files picker picks: a card or folder for the import
    /// screen, or photo files to import straight away.
    @State private var pickFolder = false
    /// The card or folder the import screen shows.
    @State private var importing: ImportChoice?
    /// Select mode: taps choose photos instead of opening them.
    @State private var selecting = false
    @State private var selected: Set<Int64> = []
    @State private var pasteNote: String?
    @State private var exporting = false
    /// The catalog sidebar, hidden while a photo is open in the editor.
    @State private var columns: NavigationSplitViewVisibility = .automatic

    var body: some View {
        NavigationSplitView(columnVisibility: $columns) {
            LibrarySidebar(model: model)
        } detail: {
        NavigationStack(path: $path) {
            ScrollView {
                FilterBar(model: model)
                    .padding(.horizontal)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 12)], spacing: 12) {
                    ForEach(model.shown, id: \.id) { photo in
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
                            .contextMenu { PhotoMenu(photo: photo, model: model) }
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
                        Text("Import RAW files from the Files app or the Photos library. They're copied into the catalog.")
                    } actions: {
                        Button("From a Card or Folder…") { pickFolder = true; picking = true }.buttonStyle(.borderedProminent)
                        Button("From Files…") { pickFolder = false; picking = true }
                        Button("From Photos…") { pickingPhotos = true }
                    }
                }
            }
            .navigationTitle("Library")
            .toolbar {
                ToolbarItem(placement: .status) {
                    if let importing = model.importing {
                        HStack {
                            ProgressView(); Text(importing)
                            if let job = model.importJob { Button("Cancel") { job.cancel() } }
                        }
                    } else if let notice = model.notice {
                        Text(notice).foregroundStyle(.secondary).lineLimit(1)
                    } else if let pasteNote {
                        Text(pasteNote).foregroundStyle(.secondary).lineLimit(1)
                    } else {
                        Text("\(model.photos.count) photos").foregroundStyle(.secondary).fixedSize()
                    }
                }
                ToolbarItem(placement: .primaryAction) {
                    Menu("Import", systemImage: "square.and.arrow.down") {
                        Button("From a Card or Folder…", systemImage: "sdcard") { pickFolder = true; picking = true }
                        Button("From Files…", systemImage: "doc") { pickFolder = false; picking = true }
                        Button("From Photos…", systemImage: "photo.on.rectangle") { pickingPhotos = true }
                    }
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
                        Button("Info", systemImage: "info.circle") { model.info = InfoRequest(ids: Array(selected)) }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Menu("Files", systemImage: "folder") { FileMenu(ids: Array(selected), photo: nil, model: model) }
                            .disabled(selected.isEmpty)
                    }
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
                    ExportSheet(library: library, ids: model.shown.map(\.id).filter(selected.contains))
                }
            }
            .fileImporter(
                isPresented: $picking,
                allowedContentTypes: pickFolder ? [.folder] : [.rawImage, .image],
                allowsMultipleSelection: !pickFolder
            ) { result in
                guard case .success(let files) = result else { return }
                if pickFolder, let folder = files.first {
                    // Kept open while the screen lists it; the import opens it again.
                    _ = folder.startAccessingSecurityScopedResource()
                    importing = ImportChoice(folder: folder)
                } else {
                    Task { await model.importFiles(files) }
                }
            }
            .sheet(item: $importing, onDismiss: nil) { choice in
                ImportScreen(model: model, choice: choice)
                    .onDisappear { choice.folder.stopAccessingSecurityScopedResource() }
            }
            .sheet(isPresented: $pickingPhotos) {
                PhotosRawPicker { results in
                    pickingPhotos = false
                    Task { await model.importFromPhotos(results) }
                }
                .ignoresSafeArea()
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
        }
        .sheet(item: Bindable(model).info) { request in InfoSheet(model: model, ids: request.ids) }
        .modifier(FileAlerts(model: model))
        .modifier(CopyAlerts(model: model, renaming: Bindable(model).renamingCopy, removing: Bindable(model).removingCopy))
        .onChange(of: path) { _, open in columns = open.isEmpty ? .automatic : .detailOnly }
        .task {
            await model.open()
            let args = ProcessInfo.processInfo.arguments
            if args.contains("-import-documents") { await model.importDocuments() }
            if args.contains("-photos-import-test") { await model.photosImportTest() }
            if args.contains("-import-screen-test") { await model.importScreenTest() }
            if args.contains("-sort-test") { model.sortTest() }
            if args.contains("-metadata-test") { model.metadataTest() }
            if args.contains("-virtual-copy-test") { await model.virtualCopyTest() }
            if args.contains("-file-ops-test") { await model.fileOpsTest() }
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
extension ImportChoice: Identifiable {}

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
            HStack(spacing: 4) {
                if !photo.label.isEmpty {
                    Circle().fill(labelColor(photo.label)).frame(width: 8, height: 8)
                }
                if photo.flag != 0 {
                    Image(systemName: photo.flag > 0 ? "flag.fill" : "xmark.circle.fill")
                        .font(.caption2).foregroundStyle(photo.flag > 0 ? Color.primary : Color.red)
                }
                Text(photo.title).font(.caption).lineLimit(1)
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

/// A photo's long-press menu: rating, flag, label and Quick Collection.
struct PhotoMenu: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    var body: some View {
        Menu("Rating") {
            ForEach(0...5, id: \.self) { n in
                Button(n == 0 ? "No rating" : String(repeating: "★", count: n)) {
                    model.setRating(photo, to: Int32(n))
                }
            }
        }
        Menu("Flag") {
            Button("Pick", systemImage: "flag.fill") { model.setFlag(photo, to: 1) }
            Button("Reject", systemImage: "xmark.circle") { model.setFlag(photo, to: -1) }
            Button("Unflag", systemImage: "flag.slash") { model.setFlag(photo, to: 0) }
        }
        Menu("Label") {
            ForEach(labels, id: \.self) { label in
                Button(label) { model.setLabel(photo, to: label) }
            }
            Button("None") { model.setLabel(photo, to: "") }
        }
        Button("Info…", systemImage: "info.circle") { model.info = InfoRequest(ids: [photo.id]) }
        CopyMenu(photo: photo, model: model, renaming: Bindable(model).renamingCopy, removing: Bindable(model).removingCopy)
        Divider()
        FileMenu(ids: [photo.id], photo: photo, model: model)
        if model.inQuickCollection(photo) {
            Button("Remove from Quick Collection", systemImage: "circle.slash") { model.setQuickCollection([photo.id], false) }
        } else {
            Button("Add to Quick Collection", systemImage: "circle.inset.filled") { model.setQuickCollection([photo.id], true) }
        }
    }
}

/// Lightroom's colour labels.
let labels = ["Red", "Yellow", "Green", "Blue", "Purple"]

func labelColor(_ label: String) -> Color {
    switch label {
    case "Red": .red
    case "Yellow": .yellow
    case "Green": .green
    case "Blue": .blue
    case "Purple": .purple
    default: .gray
    }
}

/// All Photographs, the Quick Collection and the catalog's collections.
struct LibrarySidebar: View {
    let model: LibraryModel

    private var selection: Binding<Int64?> {
        Binding(
            get: { model.collection ?? -1 },
            set: { model.collection = ($0 == nil || $0 == -1) ? nil : $0 }
        )
    }

    var body: some View {
        List(selection: selection) {
            Label("All Photographs", systemImage: "photo.on.rectangle").tag(Int64?.some(-1))
            SwiftUI.Section("Collections") {
                ForEach(model.collections, id: \.id) { c in
                    CollectionRow(collection: c).tag(Int64?.some(c.id))
                }
            }
            if model.deletedFiles > 0 {
                SwiftUI.Section {
                    HStack {
                        Label("Recently Deleted (\(model.deletedFiles))", systemImage: "trash")
                        Spacer()
                        Button("Empty") { model.fileRequest = .emptyDeleted }
                    }
                }
            }
        }
        .navigationTitle("Catalog")
    }
}

struct CollectionRow: View {
    let collection: CollectionInfo
    var body: some View {
        Label(
            "\(collection.name) (\(collection.photos.count))",
            systemImage: collection.quick ? "circle.inset.filled" : "rectangle.stack")
    }
}

/// Lightroom's Library Filter: rating, flag and label, and the order.
struct FilterBar: View {
    @Bindable var model: LibraryModel
    var body: some View {
        HStack(spacing: 16) {
            Picker("Rating", selection: $model.minRating) {
                Text("Any rating").tag(Int32(0))
                ForEach(1...5, id: \.self) { n in Text("≥ " + String(repeating: "★", count: n)).tag(Int32(n)) }
            }
            Picker("Flag", selection: $model.flagFilter) {
                Text("Any flag").tag(Int32?.none)
                Text("Picked").tag(Int32?.some(1))
                Text("Unflagged").tag(Int32?.some(0))
                Text("Rejected").tag(Int32?.some(-1))
            }
            Picker("Label", selection: $model.labelFilter) {
                Text("Any label").tag(String?.none)
                ForEach(labels, id: \.self) { Text($0).tag(String?.some($0)) }
            }
            Spacer()
            Picker("Sort", selection: $model.sort) {
                ForEach(sortOrders(), id: \.index) { Text($0.name).tag($0.index) }
            }
            Button(model.reverse ? "Descending" : "Ascending",
                   systemImage: model.reverse ? "arrow.down" : "arrow.up") { model.reverse.toggle() }
                .labelStyle(.iconOnly)
        }
        .pickerStyle(.menu)
    }
}

/// The live editor for one photo of the catalog, with its rating. Leaving
/// saves the edit first; if that fails, the editor stays and says why.
struct EditorView: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var live = LiveModel()
    @State private var saveFailed = false
    /// The move to another photo a failed save held back; `nil`, leaving.
    @State private var heldMove: Int?
    /// The photo on screen: the one opened, then wherever a swipe went.
    @State private var current: LibraryPhoto?
    @Environment(\.dismiss) private var dismiss

    private var shown: LibraryPhoto { current ?? photo }
    private var rating: Int32 { model.photos.first { $0.id == shown.id }?.rating ?? shown.rating }
    private var flag: Int32 { model.photos.first { $0.id == shown.id }?.flag ?? shown.flag }
    /// Where the photo is in the grid's order, as the grid's filters and sort
    /// have it now.
    private var position: (index: Int, count: Int)? {
        let order = model.shown
        return order.firstIndex { $0.id == shown.id }.map { ($0, order.count) }
    }

    var body: some View {
        Group {
            if let library = model.library {
                LiveEditor(photo: shown, library: library, model: live)
            }
        }
        .navigationTitle(shown.title)
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden()
        .onAppear { live.onSwipe = { go(by: $0) } }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button("Library", systemImage: "chevron.backward") {
                    heldMove = nil
                    if live.save() { dismiss() } else { saveFailed = true }
                }
            }
            ToolbarItem(placement: .navigation) {
                HStack(spacing: 2) {
                    Button("Previous Photo", systemImage: "chevron.left") { go(by: -1) }
                        .disabled((position?.index ?? 0) == 0)
                    if let position { Text("\(position.index + 1) of \(position.count)").font(.caption).monospacedDigit() }
                    Button("Next Photo", systemImage: "chevron.right") { go(by: 1) }
                        .disabled(position.map { $0.index + 1 >= $0.count } ?? true)
                }
                .labelStyle(.iconOnly)
            }
            ToolbarItem(placement: .primaryAction) {
                HStack(spacing: 2) {
                    Button("Info", systemImage: "info.circle") { model.info = InfoRequest(ids: [shown.id]) }
                        .labelStyle(.iconOnly)
                    Button("Create Virtual Copy", systemImage: "plus.square.on.square") { copyAndOpen() }
                        .labelStyle(.iconOnly)
                    Divider().frame(height: 20)
                    Button { model.setFlag(shown, to: flag == 1 ? 0 : 1) } label: {
                        Image(systemName: flag == 1 ? "flag.fill" : "flag")
                    }
                    Button { model.setFlag(shown, to: flag == -1 ? 0 : -1) } label: {
                        Image(systemName: flag == -1 ? "xmark.circle.fill" : "xmark.circle")
                            .foregroundStyle(flag == -1 ? .red : .secondary)
                    }
                    Divider().frame(height: 20)
                    ForEach(1...5, id: \.self) { n in
                        Button {
                            model.setRating(shown, to: rating == n ? 0 : Int32(n))
                        } label: {
                            Image(systemName: n <= rating ? "star.fill" : "star")
                                .foregroundStyle(n <= rating ? .yellow : .secondary)
                        }
                    }
                }
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willResignActiveNotification)) { _ in
            live.save()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.didEnterBackgroundNotification)) { _ in
            live.save()
        }
        .alert("The edit couldn't be saved", isPresented: $saveFailed) {
            Button("Try Again") {
                if let by = heldMove { go(by: by) } else if live.save() { dismiss() } else { saveFailed = true }
            }
            Button("Stay", role: .cancel) {}
        } message: {
            Text(live.edit?.saveError ?? "")
        }
    }

    /// Lightroom's Create Virtual Copy from the editor: saves, makes the copy
    /// and goes on editing it.
    private func copyAndOpen() {
        guard live.save() else { heldMove = nil; saveFailed = true; return }
        Task { if let copy = await model.createVirtualCopy(of: shown) { current = copy } }
    }

    /// Moves `by` photos along the grid's order, saving this one's edit
    /// first: a failed save stays here and says so.
    private func go(by: Int) {
        guard let position, model.shown.indices.contains(position.index + by) else { return }
        guard live.save() else { heldMove = by; saveFailed = true; return }
        heldMove = nil
        current = model.shown[position.index + by]
    }
}

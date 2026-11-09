import SwiftUI

/// What a photo's menu asked to do to its file, waiting for a name or a yes.
enum FileRequest: Identifiable {
    case rename(LibraryPhoto)
    case remove([Int64])
    case delete([Int64])
    case emptyDeleted

    var id: String {
        switch self {
        case .rename(let p): "rename-\(p.id)"
        case .remove(let ids): "remove-\(ids)"
        case .delete(let ids): "delete-\(ids)"
        case .emptyDeleted: "empty"
        }
    }
}

extension LibraryModel {
    /// Runs a file operation and says what it did, or what it left alone.
    func fileChange(_ verb: String, _ work: @escaping @Sendable (Library) throws -> FileChange) async {
        guard let library else { return }
        let outcome = await Task.detached { Result { try work(library) } }.value
        switch outcome {
        case .success(let change):
            notice = "\(verb) \(change.changed) photo\(change.changed == 1 ? "" : "s")"
                + (change.problems.isEmpty ? "" : " · " + change.problems.joined(separator: " · "))
        case .failure(let error):
            self.error = "\(error)"
        }
        await reload()
        deletedFiles = library.recentlyDeletedSize().first ?? 0
    }

    /// `-file-ops-test`: makes a virtual copy of the test copy, moves the
    /// test copy into a new folder and back, renames it and back, then
    /// deletes the virtual copy alone: no file is deleted. Prints `FILES …`.
    func fileOpsTest() async {
        guard let library, let photo = photos.first(where: { $0.filename.hasPrefix("AITEST") && !$0.isCopy }),
              let copy = await createVirtualCopy(of: photo) else { print("FILES no test copy"); return }
        let home = URL(fileURLWithPath: photo.path).deletingLastPathComponent().path
            .replacingOccurrences(of: URL(fileURLWithPath: library.photosFolder()).resolvingSymlinksInPath().path + "/", with: "")
            .replacingOccurrences(of: library.photosFolder() + "/", with: "")
        var steps: [String] = []
        func step(_ name: String, _ work: () throws -> FileChange) {
            do { let c = try work(); steps.append("\(name) \(c.changed)\(c.problems.isEmpty ? "" : " \(c.problems)")") } catch { steps.append("\(name) failed: \(error)") }
        }
        let stem = (photo.filename as NSString).deletingPathExtension
        step("new folder") { try library.newFolder(parent: "", name: "Safelight Test", photos: [photo.id]) }
        step("back") { try library.movePhotos(photos: [photo.id], folder: home) }
        step("rename") { try library.renamePhoto(photo: photo.id, stem: stem + "-renamed") }
        step("rename back") { try library.renamePhoto(photo: photo.id, stem: stem) }
        step("remove copy") { try library.removeFromCatalog(photos: [copy.id]) }
        await reload()
        let now = photos.first { $0.id == photo.id }
        print("FILES \(steps.joined(separator: "; ")); test copy at \(now?.path.hasSuffix("\(home)/\(photo.filename)") ?? false ? "its folder again" : now?.path ?? "gone"); photos \(photos.count)")
    }
}

/// The file items of a photo's menu, for one photo or the chosen ones.
struct FileMenu: View {
    let ids: [Int64]
    let photo: LibraryPhoto?
    let model: LibraryModel

    var body: some View {
        if let photo, !photo.isCopy {
            Button("Rename…", systemImage: "character.cursor.ibeam") { model.fileRequest = .rename(photo) }
        }
        Button("Remove from Library…", systemImage: "minus.circle") { model.fileRequest = .remove(ids) }
        Button("Delete…", systemImage: "trash", role: .destructive) { model.fileRequest = .delete(ids) }
    }
}

/// The names and confirmations file operations ask for.
struct FileAlerts: ViewModifier {
    let model: LibraryModel
    @State private var name = ""

    private var shown: Binding<Bool> {
        Binding(get: { model.fileRequest != nil }, set: { if !$0 { model.fileRequest = nil } })
    }

    func body(content: Content) -> some View {
        content
            .alert(title, isPresented: Binding(get: { asksName && shown.wrappedValue }, set: { if !$0 { model.fileRequest = nil } })) {
                TextField("Name", text: $name)
                Button("Cancel", role: .cancel) {}
                Button("OK") { run() }
            }
            .confirmationDialog(title, isPresented: Binding(get: { !asksName && shown.wrappedValue }, set: { if !$0 { model.fileRequest = nil } }), titleVisibility: .visible) {
                Button(confirm, role: .destructive) { run() }
            } message: {
                Text(message)
            }
            .onChange(of: model.fileRequest?.id) {
                if case .rename(let p) = model.fileRequest { name = (p.filename as NSString).deletingPathExtension } else { name = "" }
            }
    }

    private var asksName: Bool {
        switch model.fileRequest {
        case .rename: true
        default: false
        }
    }

    private var title: String {
        switch model.fileRequest {
        case .rename: "Rename Photo"
        case .remove(let ids): "Remove \(ids.count) photo\(ids.count == 1 ? "" : "s") from the library?"
        case .delete(let ids): "Delete \(ids.count) photo\(ids.count == 1 ? "" : "s")?"
        case .emptyDeleted: "Empty Recently Deleted?"
        case nil: ""
        }
    }

    private var confirm: String {
        switch model.fileRequest {
        case .remove: "Remove from Library"
        case .delete: "Delete"
        case .emptyDeleted: "Delete for Good"
        default: "OK"
        }
    }

    private var message: String {
        switch model.fileRequest {
        case .remove: "Their edits and metadata go; their files stay on this device."
        case .delete: "Their files go to Recently Deleted, with their virtual copies; the edits go."
        case .emptyDeleted: "\(model.deletedFiles) file\(model.deletedFiles == 1 ? "" : "s") will be deleted. This can't be undone."
        default: ""
        }
    }

    private func run() {
        guard let request = model.fileRequest else { return }
        let name = name
        Task {
            switch request {
            case .rename(let p): await model.fileChange("Renamed") { try $0.renamePhoto(photo: p.id, stem: name) }
            case .remove(let ids): await model.fileChange("Removed") { try $0.removeFromCatalog(photos: ids) }
            case .delete(let ids): await model.fileChange("Deleted") { try $0.deletePhotos(photos: ids) }
            case .emptyDeleted:
                await model.fileChange("Kept") { library in try library.emptyRecentlyDeleted(); return FileChange(changed: 0, problems: []) }
            }
        }
    }
}

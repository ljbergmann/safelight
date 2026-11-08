import SwiftUI

extension LibraryPhoto {
    /// The file name, with a virtual copy's name after it.
    var title: String { isCopy ? "\(filename) · \(copyName)" : filename }
}

extension LibraryModel {
    /// Lightroom's Create Virtual Copy; returns the copy, listed.
    @discardableResult
    func createVirtualCopy(of photo: LibraryPhoto) async -> LibraryPhoto? {
        guard let library else { return nil }
        do {
            let id = try library.createVirtualCopy(id: photo.id)
            await reload()
            return photos.first { $0.id == id }
        } catch {
            self.error = "\(error)"
            return nil
        }
    }

    func renameCopy(_ photo: LibraryPhoto, to name: String) async {
        await change { try $0.renameVirtualCopy(id: photo.id, name: name) }
    }

    func setCopyAsMaster(_ photo: LibraryPhoto) async {
        await change { try $0.setCopyAsMaster(id: photo.id) }
    }

    func removeCopy(_ photo: LibraryPhoto) async {
        await change { try $0.removeVirtualCopy(id: photo.id) }
    }

    private func change(_ work: (Library) throws -> Void) async {
        guard let library else { return }
        do { try work(library) } catch { self.error = "\(error)" }
        await reload()
    }

    /// `-virtual-copy-test`: makes a virtual copy of the test copy (a photo
    /// named `AITEST-…`), renames it, then removes it. Prints `COPY …`.
    func virtualCopyTest() async {
        guard let photo = photos.first(where: { $0.filename.hasPrefix("AITEST") && !$0.isCopy }) else {
            print("COPY no test copy"); return
        }
        let before = photos.count
        guard let copy = await createVirtualCopy(of: photo) else { print("COPY failed: \(error ?? "?")"); return }
        await renameCopy(copy, to: "Safelight test")
        let renamed = photos.first { $0.id == copy.id }
        await removeCopy(copy)
        print("COPY made \(copy.title) (same file \(copy.path == photo.path)), renamed \(renamed?.title ?? "?"); photos \(before) -> \(before + 1) -> \(photos.count)")
    }
}

/// A virtual copy's menu items: rename, make it the master, remove it.
struct CopyMenu: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @Binding var renaming: LibraryPhoto?
    @Binding var removing: LibraryPhoto?

    var body: some View {
        Button("Create Virtual Copy", systemImage: "plus.square.on.square") {
            Task { await model.createVirtualCopy(of: photo) }
        }
        if photo.isCopy {
            Button("Rename Copy…", systemImage: "pencil") { renaming = photo }
            Button("Set Copy as Master", systemImage: "star.square.on.square") {
                Task { await model.setCopyAsMaster(photo) }
            }
            Button("Remove Copy…", systemImage: "trash", role: .destructive) { removing = photo }
        }
    }
}

/// The alerts the copy menu opens: a name for a copy, and removing one.
struct CopyAlerts: ViewModifier {
    let model: LibraryModel
    @Binding var renaming: LibraryPhoto?
    @Binding var removing: LibraryPhoto?
    @State private var name = ""

    func body(content: Content) -> some View {
        content
            .alert("Rename Copy", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Name", text: $name)
                Button("Cancel", role: .cancel) {}
                Button("Rename") { if let photo = renaming { Task { await model.renameCopy(photo, to: name) } } }
            }
            .onChange(of: renaming?.id) { name = renaming?.copyName ?? "" }
            .confirmationDialog(
                "Remove \(removing?.title ?? "")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
                titleVisibility: .visible
            ) {
                Button("Remove Copy", role: .destructive) { if let photo = removing { Task { await model.removeCopy(photo) } } }
            } message: {
                Text("The copy's edit and metadata go; the file and its other photos stay.")
            }
    }
}

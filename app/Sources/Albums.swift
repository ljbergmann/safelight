import SwiftUI

/// Albums and their folders, as Lightroom for iPad organises a library: an
/// album (the catalog's collection) holds photos, a folder (a collection
/// set) holds albums and folders. Nothing moves on disk.

/// What an album action asks for: a name, or a confirmation.
enum AlbumRequest: Identifiable {
    /// A new album, or with `folder` a new folder, inside `parent`; a new
    /// album gets `adding`.
    case create(parent: Int64?, folder: Bool, adding: [Int64])
    case rename(CollectionInfo)
    case delete(CollectionInfo)

    var id: String {
        switch self {
        case .create(let parent, let folder, let adding): "create-\(parent ?? 0)-\(folder)-\(adding)"
        case .rename(let c): "rename-\(c.id)"
        case .delete(let c): "delete-\(c.id)"
        }
    }
}

extension LibraryModel {
    /// Runs an album change, then lists the library again.
    func albumChange(_ work: @escaping @Sendable (Library) throws -> Void) {
        guard let library else { return }
        Task {
            let outcome = await Task.detached { Result { try work(library) } }.value
            if case .failure(RenderError.Failed(let message)) = outcome { error = message }
            else if case .failure(let e) = outcome { error = "\(e)" }
            await reload()
        }
    }

    /// The albums (not folders, nor the Quick Collection), by their path of
    /// folders: "Trips › Japan".
    var albumsByPath: [(CollectionInfo, String)] {
        collections.filter { !$0.folder && !$0.quick }
            .map { ($0, albumPath($0)) }
            .sorted { $0.1.localizedStandardCompare($1.1) == .orderedAscending }
    }

    func albumPath(_ c: CollectionInfo) -> String {
        var names = [c.name]
        var parent = c.parent
        var seen: Set<Int64> = []
        while let p = parent, seen.insert(p).inserted, let folder = collections.first(where: { $0.id == p }) {
            names.insert(folder.name, at: 0)
            parent = folder.parent
        }
        return names.joined(separator: " › ")
    }

    /// The folders `c` may move into: every folder but itself and those it
    /// holds.
    func folders(outside c: CollectionInfo) -> [(CollectionInfo, String)] {
        collections.filter { f in
            guard f.folder, f.id != c.id else { return false }
            var parent = f.parent
            var seen: Set<Int64> = []
            while let p = parent, seen.insert(p).inserted {
                if p == c.id { return false }
                parent = collections.first { $0.id == p }?.parent
            }
            return true
        }
        .map { ($0, albumPath($0)) }
        .sorted { $0.1.localizedStandardCompare($1.1) == .orderedAscending }
    }
}

/// The sidebar's Albums: the Quick Collection, then folders (which open and
/// close) and albums, folders first, each by name.
struct AlbumsSection: View {
    @Bindable var model: LibraryModel
    var chosen: () -> Void = {}
    @State private var open: Set<Int64> = []

    var body: some View {
        HStack {
            Text("Albums").font(.title3)
            Spacer()
            Menu {
                Button("New Album…", systemImage: "rectangle.stack.badge.plus") {
                    model.albumRequest = .create(parent: nil, folder: false, adding: [])
                }
                Button("New Folder…", systemImage: "folder.badge.plus") {
                    model.albumRequest = .create(parent: nil, folder: true, adding: [])
                }
            } label: {
                Image(systemName: "plus").font(.body.weight(.semibold)).frame(width: 32, height: 32)
            }
            .accessibilityLabel("New Album or Folder")
        }
        .padding(.horizontal, 12).padding(.top, 20).padding(.bottom, 6)
        if let quick = model.collections.first(where: \.quick) {
            albumRow(quick, depth: 0)
        }
        ForEach(rows(parent: nil, depth: 0), id: \.0.id) { c, depth in
            if c.folder { folderRow(c, depth: depth) } else { albumRow(c, depth: depth) }
        }
        if model.collections.allSatisfy(\.quick) {
            Text("Make an album with +, or add photos to one from their menu.")
                .font(.caption).foregroundStyle(Theme.secondaryText)
                .padding(.horizontal, 12).padding(.top, 4)
        }
    }

    /// What shows under `parent`: folders first, then albums, each by name,
    /// and inside each open folder what it holds.
    private func rows(parent: Int64?, depth: Int) -> [(CollectionInfo, Int)] {
        let here = model.collections.filter { !$0.quick && $0.parent == parent }
            .sorted { ($0.folder ? 0 : 1, $0.name.lowercased()) < ($1.folder ? 0 : 1, $1.name.lowercased()) }
        return here.flatMap { c -> [(CollectionInfo, Int)] in
            guard c.folder, open.contains(c.id), depth < 12 else { return [(c, depth)] }
            return [(c, depth)] + rows(parent: c.id, depth: depth + 1)
        }
    }

    private func albumRow(_ c: CollectionInfo, depth: Int) -> some View {
        Button { model.source = .collection(c.id); chosen() } label: {
            label(c.quick ? "circle.inset.filled" : "rectangle.stack", c.name, detail: c.photos.count.formatted(),
                  chosen: model.source == .collection(c.id), depth: depth)
        }
        .buttonStyle(.plain)
        .contextMenu { if !c.quick { menu(c) } }
    }

    private func folderRow(_ c: CollectionInfo, depth: Int) -> some View {
        let isOpen = open.contains(c.id)
        let inside = model.collections.filter { $0.parent == c.id }.count
        return Button {
            withAnimation(.snappy(duration: 0.2)) {
                if isOpen { open.remove(c.id) } else { open.insert(c.id) }
            }
        } label: {
            label(isOpen ? "folder.fill" : "folder", c.name,
                  detail: inside == 1 ? "1 item" : "\(inside) items", chosen: false, depth: depth,
                  chevron: isOpen ? "chevron.down" : "chevron.right")
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button("New Album Here…", systemImage: "rectangle.stack.badge.plus") {
                model.albumRequest = .create(parent: c.id, folder: false, adding: [])
            }
            Button("New Folder Here…", systemImage: "folder.badge.plus") {
                model.albumRequest = .create(parent: c.id, folder: true, adding: [])
            }
            Divider()
            menu(c)
        }
    }

    @ViewBuilder private func menu(_ c: CollectionInfo) -> some View {
        Button("Rename…", systemImage: "character.cursor.ibeam") { model.albumRequest = .rename(c) }
        Menu("Move to", systemImage: "folder") {
            if c.parent != nil {
                Button("Top Level") { model.albumChange { try $0.moveAlbum(id: c.id, parent: nil) } }
            }
            ForEach(model.folders(outside: c), id: \.0.id) { folder, path in
                Button(path) { model.albumChange { try $0.moveAlbum(id: c.id, parent: folder.id) } }
                    .disabled(folder.id == c.parent)
            }
        }
        Button("Delete…", systemImage: "trash", role: .destructive) { model.albumRequest = .delete(c) }
    }

    private func label(_ icon: String, _ name: String, detail: String, chosen: Bool, depth: Int,
                       chevron: String? = nil) -> some View {
        HStack(spacing: 12) {
            IconTile(systemImage: icon)
            VStack(alignment: .leading, spacing: 1) {
                Text(name).lineLimit(1)
                Text(detail).font(.caption).foregroundStyle(Theme.secondaryText)
            }
            Spacer(minLength: 0)
            if let chevron {
                Image(systemName: chevron).font(.caption.weight(.semibold)).foregroundStyle(Theme.secondaryText)
            }
        }
        .padding(8)
        .padding(.leading, CGFloat(depth) * 18)
        .background(chosen ? Theme.chosen : .clear, in: RoundedRectangle(cornerRadius: 10))
        .contentShape(Rectangle())
    }
}

/// Add to Album for photos `ids`: a new album, or one of the albums; and,
/// while an album is shown, Remove from Album.
struct AlbumMenu: View {
    let ids: [Int64]
    let model: LibraryModel

    var body: some View {
        Menu("Add to Album", systemImage: "rectangle.stack.badge.plus") {
            Button("New Album…", systemImage: "plus") {
                model.albumRequest = .create(parent: nil, folder: false, adding: ids)
            }
            Divider()
            ForEach(model.albumsByPath, id: \.0.id) { album, path in
                Button(path) { model.albumChange { try $0.changeAlbum(album: album.id, ids: ids, add: true) } }
            }
        }
        if case .collection(let id) = model.source,
           let album = model.collections.first(where: { $0.id == id }), !album.quick, !album.folder {
            Button("Remove from Album", systemImage: "rectangle.stack.badge.minus") {
                model.albumChange { try $0.changeAlbum(album: id, ids: ids, add: false) }
            }
        }
    }
}

/// The names and confirmation album actions ask for.
struct AlbumAlerts: ViewModifier {
    let model: LibraryModel
    @State private var name = ""

    private var asking: Binding<Bool> {
        Binding(get: { if case .delete = model.albumRequest { false } else { model.albumRequest != nil } },
                set: { if !$0 { model.albumRequest = nil } })
    }

    private var deleting: Binding<Bool> {
        Binding(get: { if case .delete = model.albumRequest { true } else { false } },
                set: { if !$0 { model.albumRequest = nil } })
    }

    func body(content: Content) -> some View {
        content
            .alert(title, isPresented: asking) {
                TextField("Name", text: $name)
                Button("Cancel", role: .cancel) {}
                Button(confirmName) { run() }
            }
            .confirmationDialog(title, isPresented: deleting, titleVisibility: .visible) {
                Button("Delete", role: .destructive) { run() }
            } message: {
                Text(deleteMessage)
            }
            .onChange(of: model.albumRequest?.id) {
                if case .rename(let c) = model.albumRequest { name = c.name } else { name = "" }
            }
    }

    private var title: String {
        switch model.albumRequest {
        case .create(_, let folder, let adding):
            folder ? "New Folder" : adding.isEmpty ? "New Album" : "New Album with \(adding.count) photo\(adding.count == 1 ? "" : "s")"
        case .rename(let c): c.folder ? "Rename Folder" : "Rename Album"
        case .delete(let c): "Delete \(c.folder ? "folder" : "album") “\(c.name)”?"
        case nil: ""
        }
    }

    private var confirmName: String {
        if case .rename = model.albumRequest { "Rename" } else { "Create" }
    }

    private var deleteMessage: String {
        guard case .delete(let c) = model.albumRequest else { return "" }
        return c.folder
            ? "The albums and folders inside it are deleted too. The photos stay in your library."
            : "The photos stay in your library."
    }

    private func run() {
        let name = name
        switch model.albumRequest {
        case .create(let parent, let folder, let adding):
            model.albumChange { library in
                let id = try library.createAlbum(name: name, parent: parent, folder: folder)
                if !adding.isEmpty { try library.changeAlbum(album: id, ids: adding, add: true) }
            }
        case .rename(let c): model.albumChange { try $0.renameAlbum(id: c.id, name: name) }
        case .delete(let c): model.albumChange { try $0.deleteAlbum(id: c.id) }
        case nil: break
        }
        model.albumRequest = nil
    }
}

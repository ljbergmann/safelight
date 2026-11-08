import SwiftUI

/// The photos the Info sheet is open for.
struct InfoRequest: Identifiable {
    let id = UUID()
    let ids: [Int64]
}

/// A photo's title, caption, creator, copyright and keywords, as the
/// catalog keeps them, and its camera info; for several photos, what is
/// typed goes to all of them. A field is saved when it is left or the sheet
/// closes; an emptied field is cleared.
struct InfoSheet: View {
    let model: LibraryModel
    let ids: [Int64]
    @Environment(\.dismiss) private var dismiss
    @State private var shown: PhotoMetadata?
    @State private var fields: [MetadataField: String] = [:]
    /// The fields changed here, to save.
    @State private var edited: Set<MetadataField> = []
    @State private var newKeywords = ""
    @State private var error: String?
    @FocusState private var focused: MetadataField?

    private static let order: [(MetadataField, String)] = [
        (.title, "Title"), (.caption, "Caption"), (.creator, "Creator"), (.copyright, "Copyright"),
    ]

    var body: some View {
        NavigationStack {
            Form {
                SwiftUI.Section {
                    ForEach(Self.order, id: \.1) { field, name in
                        TextField(name, text: binding(field), axis: field == .caption ? .vertical : .horizontal)
                            .focused($focused, equals: field)
                            .onSubmit { save() }
                    }
                } footer: {
                    if ids.count > 1 { Text("What you type here goes to all \(ids.count) photos.") }
                }
                SwiftUI.Section("Keywords") {
                    if ids.count == 1 {
                        ForEach(shown?.keywords ?? [], id: \.id) { keyword in
                            HStack {
                                Text(keyword.path)
                                Spacer()
                                Button("Remove", systemImage: "minus.circle") { remove(keyword) }
                                    .labelStyle(.iconOnly).foregroundStyle(.red)
                            }
                        }
                    }
                    TextField("Add keywords: commas between, > for one inside another", text: $newKeywords)
                        .onSubmit { addKeywords() }
                }
                if ids.count == 1, let camera = shown?.camera, !camera.isEmpty {
                    SwiftUI.Section("Camera") {
                        ForEach(camera, id: \.self) { Text($0) }
                    }
                }
                if let error { Text(error).foregroundStyle(.red) }
            }
            .navigationTitle(ids.count == 1 ? "Info" : "Info for \(ids.count) Photos")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { save(); addKeywords(); dismiss() }
                }
            }
            .onChange(of: focused) { save() }
        }
        .task { load() }
    }

    private func binding(_ field: MetadataField) -> Binding<String> {
        Binding(get: { fields[field] ?? "" }, set: { fields[field] = $0; edited.insert(field) })
    }

    private func load() {
        guard ids.count == 1, let library = model.library else { return }
        do {
            let read = try library.metadata(id: ids[0])
            shown = read
            fields = [.title: read.title, .caption: read.caption, .creator: read.creator, .copyright: read.copyright]
        } catch { self.error = "\(error)" }
    }

    private func save() {
        guard let library = model.library else { return }
        for field in edited {
            do { try library.setMetadataText(ids: ids, field: field, text: fields[field] ?? "") } catch { self.error = "\(error)" }
        }
        edited = []
    }

    private func addKeywords() {
        guard let library = model.library, !newKeywords.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        do {
            try library.addKeywords(ids: ids, keywords: newKeywords)
            newKeywords = ""
        } catch { self.error = "\(error)" }
        load()
    }

    private func remove(_ keyword: KeywordInfo) {
        guard let library = model.library else { return }
        do { try library.removeKeyword(ids: ids, keyword: keyword.id) } catch { self.error = "\(error)" }
        load()
    }
}

extension LibraryModel {
    /// `-metadata-test`: on a test copy (a photo named `AITEST-…`, else the
    /// first), sets a title and a keyword, reads them back with the camera
    /// info, then takes both off again; the keyword stays in the catalog's
    /// list and the title is left cleared. Prints `METADATA …`.
    func metadataTest() {
        guard let library, let photo = photos.first(where: { $0.filename.hasPrefix("AITEST") }) ?? photos.first else { return }
        do {
            let before = try library.metadata(id: photo.id)
            try library.setMetadataText(ids: [photo.id], field: .title, text: "Safelight test")
            try library.addKeywords(ids: [photo.id], keywords: "Safelight test > Info")
            let read = try library.metadata(id: photo.id)
            try library.setMetadataText(ids: [photo.id], field: .title, text: before.title)
            for keyword in read.keywords where keyword.path.hasPrefix("Safelight test") {
                try library.removeKeyword(ids: [photo.id], keyword: keyword.id)
            }
            let after = try library.metadata(id: photo.id)
            print("METADATA \(photo.filename): title \"\(read.title)\", keywords \(read.keywords.map(\.path)); camera \(read.camera); afterwards title \"\(after.title)\", keywords \(after.keywords.map(\.path))")
        } catch {
            print("METADATA failed: \(error)")
        }
    }
}

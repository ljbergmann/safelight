import SwiftUI

/// The sidebar: where the grid's photos come from, each a row with an icon
/// tile, a name and a count.
struct LibrarySidebar: View {
    @Bindable var model: LibraryModel
    /// A row was chosen: on an iPhone, the grid shows again.
    var chosen: () -> Void = {}
    @State private var about = ProcessInfo.processInfo.arguments.contains("-about")

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 2) {
                Text("Library").font(.title2).padding(.horizontal, 12).padding(.bottom, 6)
                row(.all, "All photos", "photo", count: model.photos.count)
                row(.edited, "Edited", "pencil", count: model.editedCount)
                row(.unedited, "Unedited", "pencil.slash", count: model.photos.count - model.editedCount)
                if !model.lastImported.isEmpty {
                    row(.lastImport, "Last Import", "square.and.arrow.down", count: model.lastImported.count)
                }
                if model.deletedFiles > 0 {
                    Button { model.fileRequest = .emptyDeleted } label: {
                        rowLabel("Recently deleted", "trash", detail: "\(model.deletedFiles) · tap to empty", chosen: false)
                    }
                    .buttonStyle(.plain)
                }
                AlbumsSection(model: model, chosen: chosen)
            }
            .padding(8)
        }
        .safeAreaInset(edge: .bottom) {
            Button { about = true } label: {
                rowLabel("About Safelight", "info.circle", detail: "", chosen: false)
            }
            .buttonStyle(.plain)
            .padding(8)
            .background(Theme.panel)
        }
        .background(Theme.panel)
        .toolbar(.hidden, for: .navigationBar)
        .sheet(isPresented: $about) { AboutView() }
    }

    private func row(_ source: LibrarySource, _ name: String, _ icon: String, count: Int) -> some View {
        Button { model.source = source; chosen() } label: {
            rowLabel(name, icon, detail: count.formatted(), chosen: model.source == source)
        }
        .buttonStyle(.plain)
    }

    private func rowLabel(_ name: String, _ icon: String, detail: String, chosen: Bool) -> some View {
        HStack(spacing: 12) {
            IconTile(systemImage: icon)
            VStack(alignment: .leading, spacing: 1) {
                Text(name).lineLimit(1)
                Text(detail).font(.caption).foregroundStyle(Theme.secondaryText)
            }
            Spacer(minLength: 0)
        }
        .padding(8)
        .background(chosen ? Theme.chosen : .clear, in: RoundedRectangle(cornerRadius: 10))
        .contentShape(Rectangle())
    }
}

extension LibrarySource {
    /// The grid's title for it.
    @MainActor func title(in model: LibraryModel) -> String {
        switch self {
        case .all: "All photos"
        case .edited: "Edited"
        case .unedited: "Unedited"
        case .collection(let id): model.collections.first { $0.id == id }?.name ?? "Album"
        case .lastImport: "Last Import"
        }
    }
}

/// Photos laid out edge to edge at their own shapes, row by row, as a
/// contact sheet: each row is scaled to fill the width, the last kept at
/// the target height.
struct Mosaic {
    struct Row {
        var photos: [(photo: LibraryPhoto, width: CGFloat)]
        var height: CGFloat
    }

    /// The shape a photo is laid out at before its size is read.
    static let unknownAspect: CGFloat = 1.5

    static func rows(_ photos: [LibraryPhoto], width: CGFloat, height: CGFloat, spacing: CGFloat) -> [Row] {
        guard width > 0 else { return [] }
        var rows: [Row] = []
        var pending: [LibraryPhoto] = []
        var aspects: CGFloat = 0
        func aspect(_ p: LibraryPhoto) -> CGFloat { p.aspect > 0 ? CGFloat(p.aspect) : unknownAspect }
        func close(fill: Bool) {
            guard !pending.isEmpty else { return }
            let gaps = spacing * CGFloat(pending.count - 1)
            let h = fill ? (width - gaps) / aspects : height
            rows.append(Row(photos: pending.map { ($0, aspect($0) * h) }, height: h))
            pending = []
            aspects = 0
        }
        for photo in photos {
            pending.append(photo)
            aspects += aspect(photo)
            if aspects * height + spacing * CGFloat(pending.count - 1) >= width { close(fill: true) }
        }
        close(fill: false)
        return rows
    }
}

/// The grid: by month when sorted by capture time, else one run in the
/// chosen order.
struct MosaicGrid: View {
    let model: LibraryModel
    let selecting: Bool
    @Binding var selected: Set<Int64>
    let width: CGFloat

    private static let rowHeight: CGFloat = 180
    private static let spacing: CGFloat = 3

    private var sections: [(key: String, photos: [LibraryPhoto])] {
        let shown = model.shown
        guard model.sort == 0 else { return [("", shown)] }
        var order: [String] = []
        var byMonth: [String: [LibraryPhoto]] = [:]
        for p in shown {
            if byMonth[p.month] == nil { order.append(p.month) }
            byMonth[p.month, default: []].append(p)
        }
        // Photos with no date after the dated ones.
        let months = order.filter { !$0.isEmpty } + order.filter(\.isEmpty)
        return months.map { ($0, byMonth[$0]!) }
    }

    /// The grid's lines: month headers and rows, each with an id of its
    /// own across the whole grid.
    private enum Line: Identifiable {
        case header(key: String, count: Int)
        case row(key: String, index: Int, Mosaic.Row)

        var id: String {
            switch self {
            case .header(let key, _): "h-\(key)"
            case .row(let key, let index, _): "r-\(key)-\(index)"
            }
        }
    }

    private var lines: [Line] {
        sections.flatMap { section -> [Line] in
            let rows = Mosaic.rows(section.photos, width: width, height: Self.rowHeight, spacing: Self.spacing)
            let header: [Line] = model.sort == 0 ? [.header(key: section.key, count: section.photos.count)] : []
            return header + rows.enumerated().map { .row(key: section.key, index: $0.offset, $0.element) }
        }
    }

    var body: some View {
        LazyVStack(alignment: .leading, spacing: Self.spacing) {
            ForEach(lines) { line in
                switch line {
                case .header(let key, let count):
                    HStack {
                        Text(Self.title(of: key)).font(.title3)
                        Spacer()
                        Text(count.formatted()).foregroundStyle(Theme.secondaryText)
                    }
                    .padding(.horizontal, 16).padding(.top, 22).padding(.bottom, 8)
                case .row(_, _, let row):
                    HStack(spacing: Self.spacing) {
                        ForEach(row.photos, id: \.photo.id) { item in
                            cell(item.photo).frame(width: item.width, height: row.height)
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder private func cell(_ photo: LibraryPhoto) -> some View {
        if selecting {
            Button {
                if selected.contains(photo.id) { selected.remove(photo.id) } else { selected.insert(photo.id) }
            } label: {
                MosaicCell(photo: photo, model: model)
                    .overlay(alignment: .topTrailing) {
                        Image(systemName: selected.contains(photo.id) ? "checkmark.circle.fill" : "circle")
                            .font(.title3).foregroundStyle(.white, Theme.accent).padding(6)
                    }
            }
            .buttonStyle(.plain)
        } else {
            NavigationLink(value: photo.id) { MosaicCell(photo: photo, model: model) }
                .buttonStyle(.plain)
                .contextMenu { PhotoMenu(photo: photo, model: model) }
        }
    }

    /// "2023 April" for "2023-04"; "No date" without one.
    static func title(of month: String) -> String {
        let parse = DateFormatter()
        parse.locale = Locale(identifier: "en_US_POSIX")
        parse.dateFormat = "yyyy-MM"
        guard let date = parse.date(from: month) else { return "No date" }
        let show = DateFormatter()
        show.setLocalizedDateFormatFromTemplate("yyyyMMMM")
        return show.string(from: date)
    }
}

/// A photo in the mosaic, filling its place; its flag, label and stars
/// small in a corner when it has any.
struct MosaicCell: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var image: UIImage?

    var body: some View {
        Rectangle().fill(Theme.raised)
            .overlay {
                if let image { Image(uiImage: image).resizable().scaledToFill() }
            }
            .clipped()
            .overlay(alignment: .bottomLeading) { marks }
            .overlay(alignment: .topLeading) { StackBadge(photo: photo, model: model) }
            .contentShape(Rectangle())
            .task(id: photo.id) { image = await model.thumbnail(photo, edge: 512) }
    }

    @ViewBuilder private var marks: some View {
        if photo.rating > 0 || photo.flag != 0 || !photo.label.isEmpty || photo.isCopy {
            HStack(spacing: 4) {
                if photo.flag != 0 {
                    Image(systemName: photo.flag > 0 ? "flag.fill" : "xmark")
                        .foregroundStyle(photo.flag > 0 ? .white : Theme.accent)
                }
                if !photo.label.isEmpty { Circle().fill(labelColor(photo.label)).frame(width: 8, height: 8) }
                if photo.rating > 0 { Text(String(repeating: "★", count: Int(photo.rating))) }
                if photo.isCopy { Image(systemName: "square.on.square") }
            }
            .font(.caption2)
            .padding(.horizontal, 6).padding(.vertical, 3)
            .background(.black.opacity(0.55), in: Capsule())
            .padding(6)
        }
    }
}

/// Lightroom's Library Filter, folded into one menu: rating, flag, label.
struct FilterMenu: View {
    @Bindable var model: LibraryModel

    private var active: Bool { model.minRating > 0 || model.flagFilter != nil || model.labelFilter != nil }

    var body: some View {
        Menu {
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
            if active {
                Button("Clear Filters", systemImage: "xmark.circle") {
                    model.minRating = 0; model.flagFilter = nil; model.labelFilter = nil
                }
            }
        } label: {
            Label("Filter", systemImage: active ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease")
        }
    }
}

/// The sort order and its direction.
struct SortMenu: View {
    @Bindable var model: LibraryModel

    var body: some View {
        Menu {
            Picker("Sort", selection: $model.sort) {
                ForEach(sortOrders(), id: \.index) { Text($0.name).tag($0.index) }
            }
            Toggle("Newest First", isOn: $model.reverse)
        } label: {
            Label("Sort", systemImage: "arrow.up.arrow.down")
        }
    }
}

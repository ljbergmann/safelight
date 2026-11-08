import SwiftUI

extension LibraryPhoto {
    /// Hidden under its stack's top while the stack is collapsed.
    var hiddenInStack: Bool { stack != 0 && stackCollapsed && stackIndex > 0 }
}

extension LibraryModel {
    /// Runs a stack change, then lists the photos again.
    func changeStacks(_ work: @escaping @Sendable (Library) throws -> Void) async {
        guard let library else { return }
        do { try work(library) } catch { self.error = "\(error)" }
        await reload()
    }

    /// `-stack-test`: stacks the first three photos of the test copy's
    /// folder (or the first photo's), expands and unstacks them, printing
    /// `STACKS …`. Leaves no stack behind, unless `-keep`.
    func stackTest() async {
        guard let library else { return }
        let folder = (photos.first { $0.filename.hasPrefix("AITEST") } ?? photos.first)?.folderPath
        let chosen = photos.filter { $0.folderPath == folder && !$0.isCopy }.prefix(3).map(\.id)
        guard chosen.count == 3 else { print("STACKS not enough photos"); return }
        var steps: [String] = []
        let shownBefore = shown.count
        await changeStacks { try $0.stackPhotos(ids: Array(chosen)) }
        let top = photos.first { $0.id == chosen[0] }
        steps.append("stacked \(top?.stackCount ?? 0) under \(top?.filename ?? "?"), grid \(shownBefore) -> \(shown.count)")
        if let stack = top?.stack {
            await changeStacks { try $0.setStackCollapsed(stack: stack, collapsed: false) }
            steps.append("expanded, grid \(shown.count)")
        }
        if ProcessInfo.processInfo.arguments.contains("-keep") {
            if let stack = top?.stack { await changeStacks { try $0.setStackCollapsed(stack: stack, collapsed: true) } }
            print("STACKS \(steps.joined(separator: "; ")); kept")
            return
        }
        await changeStacks { try $0.unstack(ids: Array(chosen)) }
        steps.append("unstacked, grid \(shown.count); stacks left \(photos.filter { $0.stack != 0 }.count)")
        _ = library
        print("STACKS \(steps.joined(separator: "; "))")
    }
}

/// A photo's stack items: open or close its stack, make it the top, take
/// the stack apart.
struct StackMenu: View {
    let photo: LibraryPhoto
    let model: LibraryModel

    var body: some View {
        if photo.stack != 0 {
            Menu("Stack", systemImage: "square.stack") {
                Button(photo.stackCollapsed ? "Expand Stack" : "Collapse Stack",
                       systemImage: photo.stackCollapsed ? "arrow.up.left.and.arrow.down.right" : "arrow.down.right.and.arrow.up.left") {
                    let (stack, collapsed) = (photo.stack, !photo.stackCollapsed)
                    Task { await model.changeStacks { try $0.setStackCollapsed(stack: stack, collapsed: collapsed) } }
                }
                if photo.stackIndex > 0 {
                    Button("Move to Top of Stack", systemImage: "arrow.up.to.line") {
                        let id = photo.id
                        Task { await model.changeStacks { try $0.moveToTopOfStack(id: id) } }
                    }
                }
                Button("Unstack", systemImage: "square.3.layers.3d.down.right.slash") {
                    let id = photo.id
                    Task { await model.changeStacks { try $0.unstack(ids: [id]) } }
                }
            }
        }
    }
}

/// The Stack items for photos chosen in the grid.
struct SelectionStackMenu: View {
    let ids: [Int64]
    let model: LibraryModel

    var body: some View {
        Button("Group into Stack", systemImage: "square.stack") {
            // In the grid's order, the first the top.
            let order = model.shown.map(\.id).filter(ids.contains)
            Task { await model.changeStacks { try $0.stackPhotos(ids: order) } }
        }
        .disabled(ids.count < 2)
        Button("Unstack", systemImage: "square.3.layers.3d.down.right.slash") {
            Task { await model.changeStacks { try $0.unstack(ids: ids) } }
        }
        Menu("Auto-Stack by Capture Time", systemImage: "clock.badge") {
            ForEach([(5.0, "5 seconds apart"), (30.0, "30 seconds apart"), (60.0, "1 minute apart"), (300.0, "5 minutes apart")], id: \.0) { seconds, name in
                Button(name) {
                    Task { await model.changeStacks { _ = try $0.autoStack(ids: ids, seconds: seconds) } }
                }
            }
        }
    }
}

/// On a stack's top in the grid: how many photos it holds; a tap opens or
/// closes it. On the others of an open stack, a mark of it.
struct StackBadge: View {
    let photo: LibraryPhoto
    let model: LibraryModel

    var body: some View {
        if photo.stack != 0 {
            if photo.stackIndex == 0 {
                Button {
                    let (stack, collapsed) = (photo.stack, !photo.stackCollapsed)
                    Task { await model.changeStacks { try $0.setStackCollapsed(stack: stack, collapsed: collapsed) } }
                } label: {
                    Label("\(photo.stackCount)", systemImage: photo.stackCollapsed ? "square.stack.fill" : "square.stack")
                        .font(.caption.weight(.semibold))
                        .padding(.horizontal, 7).padding(.vertical, 4)
                        .background(photo.stackCollapsed ? Theme.accent : .black.opacity(0.6), in: Capsule())
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .padding(6)
                .accessibilityLabel(photo.stackCollapsed ? "Expand stack of \(photo.stackCount)" : "Collapse stack")
            } else {
                Text("\(photo.stackIndex + 1)/\(photo.stackCount)")
                    .font(.caption2).padding(.horizontal, 6).padding(.vertical, 3)
                    .background(.black.opacity(0.5), in: Capsule()).foregroundStyle(.white)
                    .padding(6)
            }
        }
    }
}

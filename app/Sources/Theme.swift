import SwiftUI

/// Safelight's look: a dark room, with the safelight's red as the one
/// accent. The same colours as the logo (docs/brand/logo.svg).
enum Theme {
    /// Behind photos: the grid and the editor's canvas.
    static let canvas = Color(red: 0.043, green: 0.043, blue: 0.043)
    /// Side panels: the library's sidebar, the editor's tool rail.
    static let panel = Color(red: 0.078, green: 0.075, blue: 0.075)
    /// The editor's panel, floating pills.
    static let raised = Color(red: 0.114, green: 0.106, blue: 0.106)
    /// A chosen row, a button's ground.
    static let chosen = Color(red: 0.165, green: 0.153, blue: 0.149)
    /// Icon tiles, slider tracks, rules.
    static let line = Color(red: 0.227, green: 0.208, blue: 0.2)
    /// The shade's red.
    static let accent = Color(red: 0.839, green: 0.251, blue: 0.184)
    static let secondaryText = Color(red: 0.54, green: 0.52, blue: 0.5)
}

/// An icon on a tile, as the sidebar's rows start.
struct IconTile: View {
    let systemImage: String
    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: 17))
            .frame(width: 40, height: 40)
            .background(Theme.line, in: RoundedRectangle(cornerRadius: 8))
    }
}

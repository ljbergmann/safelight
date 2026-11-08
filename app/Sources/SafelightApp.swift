import SwiftUI

@main
struct SafelightApp: App {
    var body: some Scene {
        WindowGroup {
            LibraryView()
                .preferredColorScheme(.dark)
                .tint(Theme.accent)
        }
    }
}

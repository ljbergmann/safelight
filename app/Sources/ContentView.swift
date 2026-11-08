import SwiftUI

/// Stage 0: shows what the Rust core returns, to prove the bridge and the
/// folders RAWmakase's crates pick inside the app's sandbox.
struct ContentView: View {
    private let greeting = hello()
    private let paths = folders()

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(greeting).font(.title2)
            LabeledContent("Data folder", value: paths.data)
            LabeledContent("Local data folder", value: paths.localData)
        }
        .font(.body.monospaced())
        .padding(32)
    }
}

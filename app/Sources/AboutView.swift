import SwiftUI
import UIKit

/// About Safelight: its version, what it is built on, and the licences of the
/// open-source software it ships (`app/Acknowledgements.txt`, written by
/// `scripts/acknowledgements.py`).
struct AboutView: View {
    @Environment(\.dismiss) private var dismiss

    private var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "Version \(short) (\(build))"
    }

    var body: some View {
        NavigationStack {
            List {
                SwiftUI.Section {
                    VStack(spacing: 8) {
                        Text("Safelight").font(.title.weight(.semibold))
                        Text(version).font(.callout).foregroundStyle(Theme.secondaryText)
                        Text("A RAW photo editor for the iPad, built on RAWmakase.")
                            .font(.callout).multilineTextAlignment(.center)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                }
                SwiftUI.Section {
                    NavigationLink("Acknowledgements") { AcknowledgementsView() }
                }
            }
            .navigationTitle("About")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }
}

/// The licences, as one long scrolling text: a UITextView keeps a text of
/// several hundred kilobytes smooth where SwiftUI's Text would not.
struct AcknowledgementsView: View {
    var body: some View {
        LicenceText(text: Self.text)
            .background(Theme.canvas)
            .navigationTitle("Acknowledgements")
            .navigationBarTitleDisplayMode(.inline)
    }

    private static let text: String = Bundle.main.url(forResource: "Acknowledgements", withExtension: "txt")
        .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? "The acknowledgements are missing from this build."
}

private struct LicenceText: UIViewRepresentable {
    let text: String

    func makeUIView(context: Context) -> UITextView {
        let view = UITextView()
        view.isEditable = false
        view.backgroundColor = .clear
        view.textColor = .label
        view.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        view.textContainerInset = UIEdgeInsets(top: 16, left: 12, bottom: 16, right: 12)
        view.text = text
        return view
    }

    func updateUIView(_ view: UITextView, context: Context) {}
}

import SwiftUI

extension AiModel: Identifiable {
    public var id: Self { self }
}

extension LiveModel {
    /// Selects the photo's subject, sky or background as a new mask. The
    /// model runs off the main thread; without the model installed, the
    /// download is offered first.
    func select(_ what: AiSelection) {
        let needs: AiModel = what == .sky ? .sky : .subject
        guard aiModels().first(where: { $0.model == needs })?.installed == true else {
            offeredModel = needs
            return
        }
        guard let live, selecting == nil else { return }
        selecting = what
        selectionError = nil
        Task.detached {
            let outcome = Result { try live.selectMask(what: what) }
            await MainActor.run {
                self.selecting = nil
                switch outcome {
                case .success(let index):
                    if let index {
                        self.selectedMask = index
                        self.tool = .none
                    }
                case .failure(let error):
                    self.selectionError = "\(error)"
                }
                self.refresh()
            }
        }
    }
}

/// Select Subject, Sky and Background, for the Masks panel.
struct SelectButtons: View {
    @Bindable var model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Button("Subject", systemImage: "person.crop.rectangle") { model.select(.subject) }
                Button("Sky", systemImage: "cloud.sun") { model.select(.sky) }
                Button("Background", systemImage: "mountain.2") { model.select(.background) }
            }
            .buttonStyle(.bordered).font(.callout)
            .disabled(model.selecting != nil)
            if let what = model.selecting {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text("Selecting \(String(describing: what))…")
                }
                .font(.caption).foregroundStyle(.secondary)
            } else if let error = model.selectionError {
                Text(error).font(.caption).foregroundStyle(.orange)
            }
        }
    }
}

/// Under the AI Denoise switch: what it is doing, with a bar while the model
/// runs, and the model's download when it isn't installed.
struct AiDenoiseStatus: View {
    let model: LiveModel

    var body: some View {
        if let status = model.edit?.aiDenoise {
            VStack(alignment: .leading, spacing: 4) {
                if let done = model.edit?.aiDenoiseProgress {
                    ProgressView(value: Double(done))
                        .tint(Theme.accent)
                        .animation(.linear(duration: 0.2), value: done)
                        .accessibilityLabel("AI Denoise")
                }
                Text(status).font(.caption).foregroundStyle(.secondary)
                if aiModels().first(where: { $0.model == .denoise })?.installed != true {
                    Button("Download the AI Denoise model…") { model.offeredModel = .denoise }
                        .font(.callout)
                }
            }
        }
    }
}

/// The model's credit and licence, then Accept and download, as the
/// desktop offers it; the download's progress, and Cancel.
struct ModelInstallSheet: View {
    let offered: AiModel
    let installed: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var install: ModelInstall?
    @State private var fetched: UInt64 = 0
    @State private var failure: String?

    private var info: AiModelInfo? { aiModels().first { $0.model == offered } }

    var body: some View {
        NavigationStack {
            Form {
                if let info {
                    SwiftUI.Section("Credit") { Text(info.credit).font(.callout) }
                    SwiftUI.Section("Licence") { Text(info.license).font(.callout) }
                    SwiftUI.Section {
                        if let install {
                            ProgressView(value: Double(fetched), total: Double(max(install.totalBytes(), 1)))
                            Text("\(fetched / 1_000_000) of \(install.totalBytes() / 1_000_000) MB")
                                .font(.caption).foregroundStyle(.secondary)
                        } else {
                            Button(failure == nil ? "Accept and download · \(info.megabytes) MB" : "Retry") {
                                start()
                            }
                        }
                        if let failure {
                            Text(failure).font(.caption).foregroundStyle(.orange)
                        }
                    } footer: {
                        Text("The model is downloaded once, checked, and kept on this iPad. It runs here; no photo leaves the device.")
                    }
                }
            }
            .navigationTitle(info?.name ?? "Model")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        install?.cancel()
                        dismiss()
                    }
                }
            }
        }
    }

    private func start() {
        failure = nil
        let running = ModelInstall.start(model: offered)
        install = running
        Task {
            while !running.finished() {
                fetched = running.fetchedBytes()
                try? await Task.sleep(for: .milliseconds(200))
            }
            if let error = running.error() {
                failure = error
                install = nil
            } else {
                installed()
                dismiss()
            }
        }
    }
}

import SwiftUI

/// Native macOS settings window (opened via the `Settings` scene, Cmd+,).
/// Uses the classic toolbar-tab layout with grouped forms, like Safari/Terminal.
struct TokenUsageSettingsView: View {
    @ObservedObject var preferences: TokenUsagePreferencesController

    var body: some View {
        TabView {
            CLIFiltersSettingsTab(preferences: preferences)
                .tabItem {
                    Label("CLI Filters", systemImage: "terminal")
                }

            BriefModelSettingsTab(preferences: preferences)
                .tabItem {
                    Label("Daily Brief", systemImage: "sparkles")
                }
        }
        .frame(width: 520, height: 440)
    }
}

private struct CLIFiltersSettingsTab: View {
    @ObservedObject var preferences: TokenUsagePreferencesController

    private var sources: [TokenUsageSource] {
        TokenUsageSource.allCases.filter { $0 != .all }
    }

    var body: some View {
        Form {
            Section {
                ForEach(sources) { source in
                    Toggle(isOn: Binding(
                        get: { preferences.isEnabled(source) },
                        set: { preferences.setEnabled(source, enabled: $0) }
                    )) {
                        Label {
                            Text(source.label)
                        } icon: {
                            CLISourceIcon(source: source)
                        }
                    }
                    .toggleStyle(.switch)
                }
            } header: {
                Text("Enabled CLIs")
            } footer: {
                Text("Controls dashboard display and Daily Brief generation.")
            }
        }
        .formStyle(.grouped)
    }
}

private struct CLISourceIcon: View {
    let source: TokenUsageSource

    var body: some View {
        if let imageAssetName = source.imageAssetName {
            BundledIconImage(imageAssetName: imageAssetName, padding: 1, size: 20)
                .background(
                    source.iconBadgeBackgroundColor,
                    in: RoundedRectangle(cornerRadius: 5, style: .continuous)
                )
        } else {
            Image(systemName: source.systemImage)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(source.tintColor)
                .frame(width: 20, height: 20)
                .background(
                    source.tintColor.opacity(0.12),
                    in: RoundedRectangle(cornerRadius: 5, style: .continuous)
                )
        }
    }
}

private struct BriefModelSettingsTab: View {
    @ObservedObject var preferences: TokenUsagePreferencesController

    var body: some View {
        Form {
            Section {
                TextField("Base URL", text: $preferences.briefBaseURL, prompt: Text(TokenUsagePreferencesController.defaultBriefBaseURL))
                TextField("Model", text: $preferences.briefModelId, prompt: Text(TokenUsagePreferencesController.defaultBriefModelId))
                SecureField("API Key (optional)", text: $preferences.briefApiKey, prompt: Text("Use local CPA api-keys"))
            } header: {
                Text("LLM Endpoint")
            } footer: {
                Text("Local CPA keys are read from ~/.config/cliproxyapi/config.yaml — no Keychain prompt.")
            }

            Section {
                HStack(spacing: 10) {
                    Button("Use Local CPA") {
                        _ = preferences.applyLocalCPADefaults(overwriteModel: true)
                    }
                    .help("Set Base URL + Model from CPA; API key stays in CPA config (not Keychain)")

                    Button {
                        Task { await preferences.testConnection() }
                    } label: {
                        if preferences.isTestingConnection {
                            ProgressView()
                                .controlSize(.small)
                        } else {
                            Text("Test Connection")
                        }
                    }
                    .disabled(preferences.isTestingConnection)

                    Spacer()

                    if let message = preferences.connectionTestMessage {
                        Label {
                            Text(message)
                        } icon: {
                            Image(systemName: message == "Connected" ? "checkmark.circle.fill" : "info.circle")
                                .foregroundStyle(message == "Connected" ? Color.green : Color.secondary)
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }
}

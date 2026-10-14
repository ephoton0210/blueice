// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct AssistantSettingsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var model: AssistantSettingsModel
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(BrowserStrings.text("Assistant Settings")).font(.title2)
                Spacer()
                Button(BrowserStrings.text("Refresh")) { Task { await model.refresh() } }.accessibilityIdentifier("assistant-settings-refresh")
            }
            Text(BrowserStrings.text("Models run locally. AI proposals wait for your approval. Changing these settings does not change mandatory browsing policy or extension permissions."))
                .font(.caption).foregroundStyle(.secondary)
            if let notice = model.notice {
                Text(BrowserStrings.text(AssistantConsentText.display(notice))).textSelection(.enabled).accessibilityIdentifier("assistant-settings-notice")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let state = model.state {
                        Text(BrowserStrings.format("In force: %@ · idle %llu seconds · niceness %lld", backendTitle(state.current.backend),
                            state.current.idle_timeout_secs, Int64(state.current.nice)))
                            .accessibilityIdentifier("assistant-settings-current")
                        if let pending = state.pending {
                            GroupBox(BrowserStrings.text("AI proposal — awaiting your decision")) {
                                VStack(alignment: .leading, spacing: 8) {
                                    Text(BrowserStrings.format("Proposal %llu · expires in up to %llu seconds", pending.id, pending.seconds_left))
                                    Text(pending.digest).font(.caption.monospaced()).textSelection(.enabled)
                                        .accessibilityIdentifier("assistant-proposal-digest")
                                    differences(pending.proposed.differences(from: state.current))
                                    HStack {
                                        Button(BrowserStrings.text("Deny proposal")) { Task { await model.deny() } }.accessibilityIdentifier("assistant-proposal-deny")
                                        Spacer()
                                        Button(BrowserStrings.text("Review approval…")) { model.prepareApproval() }
                                            .disabled(pending.seconds_left == 0).accessibilityIdentifier("assistant-proposal-review")
                                    }
                                }.frame(maxWidth: .infinity, alignment: .leading)
                            }.accessibilityIdentifier("assistant-proposal")
                        } else { Text(BrowserStrings.text("No proposal waiting.")).accessibilityIdentifier("assistant-proposal-empty") }
                        GroupBox(BrowserStrings.text("Your settings")) {
                            VStack(alignment: .leading, spacing: 10) {
                                Picker(BrowserStrings.text("Backend"), selection: $model.draft.backend) {
                                    Text(BrowserStrings.text("Off")).tag("none"); Text(BrowserStrings.text("Loopback server")).tag("loopback")
                                    Text(BrowserStrings.text("In-process Candle")).tag("candle"); Text(BrowserStrings.text("Both")).tag("both")
                                }.accessibilityLabel(BrowserStrings.text("Backend")).accessibilityIdentifier("assistant-backend")
                                field("Idle timeout (30–86400 seconds)", "assistant-idle", $model.draft.idle)
                                field("Memory ceiling (256–1048576 MiB; empty = unlimited)", "assistant-ceiling", $model.draft.ceiling)
                                field("Scheduling niceness (0–19)", "assistant-nice", $model.draft.nice)
                                if ["loopback", "both"].contains(model.draft.backend) {
                                    Picker(BrowserStrings.text("Provider"), selection: $model.draft.provider) {
                                        Text(BrowserStrings.text("Ollama")).tag("ollama"); Text(BrowserStrings.text("llama.cpp")).tag("llamacpp"); Text(BrowserStrings.text("Hugging Face TGI")).tag("huggingface")
                                    }.accessibilityLabel(BrowserStrings.text("Provider")).accessibilityIdentifier("assistant-provider")
                                    field("Loopback base URL", "assistant-base-url", $model.draft.baseURL)
                                    field("Model name", "assistant-model", $model.draft.model)
                                }
                                if ["candle", "both"].contains(model.draft.backend) {
                                    Text(BrowserStrings.text("In-process inference requires a build with Candle support and compatible local model files."))
                                        .font(.caption).foregroundStyle(.secondary)
                                    path("GGUF model file", "assistant-model-path", $model.draft.modelPath)
                                    path("Tokenizer JSON file", "assistant-tokenizer-path", $model.draft.tokenizerPath)
                                    field("Context (64–131072)", "assistant-context", $model.draft.context)
                                }
                                Button(BrowserStrings.text("Review changes…")) { model.prepareEdit() }.accessibilityIdentifier("assistant-settings-review")
                            }.textFieldStyle(.roundedBorder).frame(maxWidth: .infinity, alignment: .leading)
                                .disabled(model.confirmation != nil)
                        }
                    } else if model.busy { ProgressView(BrowserStrings.text("Inspecting assistant settings…")) }
                    else { Text(BrowserStrings.text("Settings unavailable. Refresh to inspect the owned launcher.")) }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            .accessibilityIdentifier("assistant-settings-editor")
            // Keep the decisive controls outside scrolling model metadata.
            // The complete bounded differences remain scrollable above them.
            if let confirmation = model.confirmation, let state = model.state {
                Divider()
                Text(confirmationTitle(confirmation)).font(.headline).accessibilityIdentifier("assistant-settings-confirmation")
                ScrollView { differences(confirmation.settings.differences(from: state.current)) }.frame(maxHeight: 140)
                HStack {
                    Button(BrowserStrings.text("Cancel")) { model.cancel() }.accessibilityIdentifier("assistant-settings-cancel")
                    Spacer()
                    Button(BrowserStrings.text("Confirm and apply")) { Task { await model.confirm() } }.accessibilityIdentifier("assistant-settings-confirm")
                }
            }
        }.padding(20).frame(minWidth: 560, minHeight: 460).disabled(model.busy)
    }
    private func confirmationTitle(_ value: AssistantSettingsConfirmation) -> String {
        switch value { case .edit: return BrowserStrings.text("Apply your reviewed settings?"); case .approve(let proposal): return BrowserStrings.format("Approve AI proposal %llu?", proposal.id) }
    }
    private func backendTitle(_ value: String) -> String {
        switch value {
        case "none": return BrowserStrings.text("Off")
        case "loopback": return BrowserStrings.text("Loopback server")
        case "candle": return BrowserStrings.text("In-process Candle")
        case "both": return BrowserStrings.text("Both")
        default: return value
        }
    }
    private func differences(_ values: [AssistantSettingDifference]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(values) { value in
                VStack(alignment: .leading, spacing: 2) {
                    Text(BrowserStrings.text(value.label)).font(.headline)
                    Text(BrowserStrings.text("Before: ") + AssistantConsentText.display(value.before)).textSelection(.enabled)
                    Text(BrowserStrings.text("After: ") + AssistantConsentText.display(value.after)).textSelection(.enabled)
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func field(_ label: String, _ id: String, _ text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(BrowserStrings.text(label)).font(.caption)
            TextField(BrowserStrings.text(label), text: text).accessibilityLabel(BrowserStrings.text(label)).accessibilityIdentifier(id)
        }
    }
    private func path(_ label: String, _ id: String, _ text: Binding<String>) -> some View {
        HStack {
            field(label, id, text)
            Button(BrowserStrings.text("Choose…")) {
                let panel = NSOpenPanel(); panel.title = BrowserStrings.text(label); panel.allowsMultipleSelection = false
                panel.canChooseDirectories = false; panel.canChooseFiles = true
                if panel.runModal() == .OK, let url = panel.url { text.wrappedValue = url.path }
            }.accessibilityLabel(BrowserStrings.format("Choose %@", BrowserStrings.text(label))).accessibilityIdentifier(id + "-choose")
        }
    }
}

struct TrustedPanelsView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var permissions: PermissionPanelModel
    @ObservedObject var assistant: AssistantSettingsModel
    @State private var panel = "permissions"
    var body: some View {
        VStack(spacing: 0) {
            Picker(BrowserStrings.text("Browser panel"), selection: $panel) {
                Text(BrowserStrings.text("Permissions")).tag("permissions"); Text(BrowserStrings.text("Assistant Settings")).tag("assistant")
            }.pickerStyle(.segmented).padding(.horizontal, 20).padding(.top, 12)
                .accessibilityIdentifier("trusted-panel-selector").disabled(permissions.busy || assistant.busy)
            if panel == "permissions" { PermissionPanelView(model: permissions) }
            else { AssistantSettingsView(model: assistant) }
        }.onChange(of: panel) { _, value in
            // A private assistant request invalidates a pending one-shot
            // review in the launcher too. Never leave an old native arm button.
            permissions.cancel(); assistant.cancel()
            Task { if value == "permissions" { await permissions.refresh() } else { await assistant.refresh() } }
        }
    }
}

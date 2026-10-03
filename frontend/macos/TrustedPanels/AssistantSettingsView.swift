// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct AssistantSettingsView: View {
    @ObservedObject var model: AssistantSettingsModel
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Assistant Settings").font(.title2)
                Spacer()
                Button("Refresh") { Task { await model.refresh() } }.accessibilityIdentifier("assistant-settings-refresh")
            }
            Text("Models run locally. AI proposals wait for your approval. Changing these settings does not change mandatory browsing policy or extension permissions.")
                .font(.caption).foregroundStyle(.secondary)
            if let notice = model.notice {
                Text(AssistantConsentText.display(notice)).textSelection(.enabled).accessibilityIdentifier("assistant-settings-notice")
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let state = model.state {
                        Text("In force: \(state.current.backend) · idle \(state.current.idle_timeout_secs) seconds · niceness \(state.current.nice)")
                            .accessibilityIdentifier("assistant-settings-current")
                        if let pending = state.pending {
                            GroupBox("AI proposal — awaiting your decision") {
                                VStack(alignment: .leading, spacing: 8) {
                                    Text("Proposal \(pending.id) · expires in up to \(pending.seconds_left) seconds")
                                    Text(pending.digest).font(.caption.monospaced()).textSelection(.enabled)
                                        .accessibilityIdentifier("assistant-proposal-digest")
                                    differences(pending.proposed.differences(from: state.current))
                                    HStack {
                                        Button("Deny proposal") { Task { await model.deny() } }.accessibilityIdentifier("assistant-proposal-deny")
                                        Spacer()
                                        Button("Review approval…") { model.prepareApproval() }
                                            .disabled(pending.seconds_left == 0).accessibilityIdentifier("assistant-proposal-review")
                                    }
                                }.frame(maxWidth: .infinity, alignment: .leading)
                            }.accessibilityIdentifier("assistant-proposal")
                        } else { Text("No proposal waiting.").accessibilityIdentifier("assistant-proposal-empty") }
                        GroupBox("Your settings") {
                            VStack(alignment: .leading, spacing: 10) {
                                Picker("Backend", selection: $model.draft.backend) {
                                    Text("Off").tag("none"); Text("Loopback server").tag("loopback")
                                    Text("In-process Candle").tag("candle"); Text("Both").tag("both")
                                }.accessibilityIdentifier("assistant-backend")
                                field("Idle timeout (30–86400 seconds)", "assistant-idle", $model.draft.idle)
                                field("Memory ceiling (256–1048576 MiB; empty = unlimited)", "assistant-ceiling", $model.draft.ceiling)
                                field("Scheduling niceness (0–19)", "assistant-nice", $model.draft.nice)
                                if ["loopback", "both"].contains(model.draft.backend) {
                                    Picker("Provider", selection: $model.draft.provider) {
                                        Text("Ollama").tag("ollama"); Text("llama.cpp").tag("llamacpp"); Text("Hugging Face TGI").tag("huggingface")
                                    }.accessibilityIdentifier("assistant-provider")
                                    field("Loopback base URL", "assistant-base-url", $model.draft.baseURL)
                                    field("Model name", "assistant-model", $model.draft.model)
                                }
                                if ["candle", "both"].contains(model.draft.backend) {
                                    Text("In-process inference requires a build with Candle support and compatible local model files.")
                                        .font(.caption).foregroundStyle(.secondary)
                                    path("GGUF model file", "assistant-model-path", $model.draft.modelPath)
                                    path("Tokenizer JSON file", "assistant-tokenizer-path", $model.draft.tokenizerPath)
                                    field("Context (64–131072)", "assistant-context", $model.draft.context)
                                }
                                Button("Review changes…") { model.prepareEdit() }.accessibilityIdentifier("assistant-settings-review")
                            }.textFieldStyle(.roundedBorder).frame(maxWidth: .infinity, alignment: .leading)
                                .disabled(model.confirmation != nil)
                        }
                    } else if model.busy { ProgressView("Inspecting assistant settings…") }
                    else { Text("Settings unavailable. Refresh to inspect the owned launcher.") }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            // Keep the decisive controls outside scrolling model metadata.
            // The complete bounded differences remain scrollable above them.
            if let confirmation = model.confirmation, let state = model.state {
                Divider()
                Text(confirmationTitle(confirmation)).font(.headline).accessibilityIdentifier("assistant-settings-confirmation")
                ScrollView { differences(confirmation.settings.differences(from: state.current)) }.frame(maxHeight: 140)
                HStack {
                    Button("Cancel") { model.cancel() }.accessibilityIdentifier("assistant-settings-cancel")
                    Spacer()
                    Button("Confirm and apply") { Task { await model.confirm() } }.accessibilityIdentifier("assistant-settings-confirm")
                }
            }
        }.padding(20).frame(minWidth: 560, minHeight: 460).disabled(model.busy)
    }
    private func confirmationTitle(_ value: AssistantSettingsConfirmation) -> String {
        switch value { case .edit: return "Apply your reviewed settings?"; case .approve(let proposal): return "Approve AI proposal \(proposal.id)?" }
    }
    private func differences(_ values: [AssistantSettingDifference]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(values) { value in
                VStack(alignment: .leading, spacing: 2) {
                    Text(value.label).font(.headline)
                    Text("Before: " + AssistantConsentText.display(value.before)).textSelection(.enabled)
                    Text("After: " + AssistantConsentText.display(value.after)).textSelection(.enabled)
                }
            }
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func field(_ label: String, _ id: String, _ text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label).font(.caption)
            TextField(label, text: text).accessibilityLabel(label).accessibilityIdentifier(id)
        }
    }
    private func path(_ label: String, _ id: String, _ text: Binding<String>) -> some View {
        HStack {
            field(label, id, text)
            Button("Choose…") {
                let panel = NSOpenPanel(); panel.title = label; panel.allowsMultipleSelection = false
                panel.canChooseDirectories = false; panel.canChooseFiles = true
                if panel.runModal() == .OK, let url = panel.url { text.wrappedValue = url.path }
            }.accessibilityLabel("Choose " + label).accessibilityIdentifier(id + "-choose")
        }
    }
}

struct TrustedPanelsView: View {
    @ObservedObject var permissions: PermissionPanelModel
    @ObservedObject var assistant: AssistantSettingsModel
    @State private var panel = "permissions"
    var body: some View {
        VStack(spacing: 0) {
            Picker("Browser panel", selection: $panel) {
                Text("Permissions").tag("permissions"); Text("Assistant Settings").tag("assistant")
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

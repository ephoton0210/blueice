// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI

struct BrowserAssistantView: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject var assistant: BrowserAssistant
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Local Assistant").font(.headline)
                Spacer()
                Button { model.assistantPresented = false } label: { Image(systemName: "xmark") }
                    .accessibilityLabel("Close assistant").accessibilityIdentifier("assistant-close")
            }
            if let tab = model.selected, let page = assistant.page(tab) {
                Text(page.url).font(.caption).lineLimit(2).textSelection(.enabled).accessibilityIdentifier("assistant-source")
                GroupBox("Page tools") {
                    VStack(alignment: .leading, spacing: 8) {
                        Button("Summarize page") { model.askAssistant() }
                            .disabled(!model.canAskAssistant || page.pending != nil).accessibilityIdentifier("assistant-summarize")
                        TextField("Organize instruction", text: Binding(get: { page.instruction }, set: { assistant.setInstruction($0, tab: tab) }))
                            .textFieldStyle(.roundedBorder).accessibilityIdentifier("assistant-instruction")
                        Button("Organize page") { model.askAssistant(organize: true) }
                            .disabled(!model.canAskAssistant || page.pending != nil).accessibilityIdentifier("assistant-organize")
                        if page.pending != nil {
                            HStack {
                                ProgressView().controlSize(.small)
                                Text("Working…").accessibilityIdentifier("assistant-working")
                                Spacer()
                                Button("Stop waiting") { assistant.stopWaiting(tab) }.accessibilityIdentifier("assistant-stop")
                            }
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
                GroupBox("Translation") {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Future navigations in all windows and profiles").font(.caption)
                        HStack {
                            TextField("Language tag", text: $assistant.languageDraft).textFieldStyle(.roundedBorder)
                                .accessibilityIdentifier("assistant-language")
                            Button("Apply") { model.changeTranslation(.language(assistant.languageDraft)) }.accessibilityIdentifier("assistant-language-apply")
                            Button("Off") { model.changeTranslation(.language(nil)) }.accessibilityIdentifier("assistant-language-off")
                        }
                        Text("Target: \(page.translation?.language ?? "Off")").font(.caption).accessibilityIdentifier("assistant-language-state")
                        Toggle("Show translated page", isOn: Binding(get: { page.translation?.shown == true }, set: { model.changeTranslation(.show($0)) }))
                            .disabled(page.translation?.available != true).accessibilityIdentifier("assistant-show-translation")
                        if page.translation?.available == false { Text("This page has no translation.").font(.caption) }
                        Button("Refresh translation state") { model.refreshAssistantTranslation() }.accessibilityIdentifier("assistant-translation-refresh")
                    }.frame(maxWidth: .infinity, alignment: .leading).disabled(!model.ready || page.translating != nil || model.status == "Loading…")
                }
                if let notice = page.notice { Text(verbatim: notice).font(.caption).textSelection(.enabled).accessibilityIdentifier("assistant-notice") }
                if let result = page.result {
                    Text(result.kind == .summary ? "Summary" : "Organized text").font(.headline).accessibilityIdentifier("assistant-result-kind")
                    Text(result.translated ? "From the translated page" : "From the original page").font(.caption)
                    ScrollView {
                        Text(verbatim: result.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier("assistant-result")
                    }.frame(maxHeight: .infinity)
                } else { Spacer(minLength: 0) }
            } else { Text("Waiting for the current document…").foregroundStyle(.secondary); Spacer() }
            Button("Model settings and permissions…") { model.openPermissions() }.accessibilityIdentifier("assistant-settings")
        }.padding(14).frame(minWidth: 280, idealWidth: 320, maxWidth: 460, maxHeight: .infinity)
            .accessibilityElement(children: .contain).accessibilityIdentifier("assistant-panel")
    }
}

struct BrowserAssistantCommands: Commands {
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandMenu("Assistant") {
            Button(model.assistantPresented ? "Hide Assistant" : "Show Assistant") { model.toggleAssistant() }
                .keyboardShortcut("a", modifiers: [.command, .shift])
            Button("Summarize Page") { model.showAndAskAssistant() }.disabled(!model.canOpenAssistantTask)
            Button("Organize Page") { model.showAndAskAssistant(organize: true) }.disabled(!model.canOpenAssistantTask)
        }
    }
}

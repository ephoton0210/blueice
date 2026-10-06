// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI

struct BrowserAssistantView: View {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var model: BrowserModel
    @ObservedObject var assistant: BrowserAssistant
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(BrowserStrings.text("Local Assistant")).font(.headline)
                Spacer()
                Button { model.assistantPresented = false } label: { Image(systemName: "xmark") }
                    .accessibilityLabel(BrowserStrings.text("Close assistant")).accessibilityIdentifier("assistant-close")
            }
            if let tab = model.selected, let page = assistant.page(tab) {
                Text(page.url).font(.caption).lineLimit(2).textSelection(.enabled).accessibilityIdentifier("assistant-source")
                GroupBox(BrowserStrings.text("Page tools")) {
                    VStack(alignment: .leading, spacing: 8) {
                        Button(BrowserStrings.text("Summarize page")) { model.askAssistant() }
                            .disabled(!model.canAskAssistant || page.pending != nil).accessibilityIdentifier("assistant-summarize")
                        TextField(BrowserStrings.text("Organize instruction"), text: Binding(get: { page.instruction }, set: { assistant.setInstruction($0, tab: tab) }))
                            .textFieldStyle(.roundedBorder).accessibilityIdentifier("assistant-instruction")
                        Button(BrowserStrings.text("Organize page")) { model.askAssistant(organize: true) }
                            .disabled(!model.canAskAssistant || page.pending != nil).accessibilityIdentifier("assistant-organize")
                        if page.pending != nil {
                            HStack {
                                ProgressView().controlSize(.small)
                                Text(BrowserStrings.text("Working…")).accessibilityIdentifier("assistant-working")
                                Spacer()
                                Button(BrowserStrings.text("Stop waiting")) { assistant.stopWaiting(tab) }.accessibilityIdentifier("assistant-stop")
                            }
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
                GroupBox(BrowserStrings.text("Translation")) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(BrowserStrings.text("Future navigations in all windows and profiles")).font(.caption)
                        HStack {
                            TextField(BrowserStrings.text("Language tag"), text: $assistant.languageDraft).textFieldStyle(.roundedBorder)
                                .accessibilityIdentifier("assistant-language")
                            Button(BrowserStrings.text("Apply")) { model.changeTranslation(.language(assistant.languageDraft)) }.accessibilityIdentifier("assistant-language-apply")
                            Button(BrowserStrings.text("Off")) { model.changeTranslation(.language(nil)) }.accessibilityIdentifier("assistant-language-off")
                        }
                        Text(BrowserStrings.format("Target: %@", page.translation?.language ?? BrowserStrings.text("Off"))).font(.caption).accessibilityIdentifier("assistant-language-state")
                        Toggle(BrowserStrings.text("Show translated page"), isOn: Binding(get: { page.translation?.shown == true }, set: { model.changeTranslation(.show($0)) }))
                            .disabled(page.translation?.available != true).accessibilityIdentifier("assistant-show-translation")
                        if page.translation?.available == false { Text(BrowserStrings.text("This page has no translation.")).font(.caption) }
                        Button(BrowserStrings.text("Refresh translation state")) { model.refreshAssistantTranslation() }.accessibilityIdentifier("assistant-translation-refresh")
                    }.frame(maxWidth: .infinity, alignment: .leading).disabled(!model.ready || page.translating != nil || model.status == "Loading…")
                }
                if let notice = page.notice { Text(verbatim: BrowserStrings.text(notice)).font(.caption).textSelection(.enabled).accessibilityIdentifier("assistant-notice") }
                if let result = page.result {
                    Text(BrowserStrings.text(result.kind == .summary ? "Summary" : "Organized text")).font(.headline).accessibilityIdentifier("assistant-result-kind")
                    Text(BrowserStrings.text(result.translated ? "From the translated page" : "From the original page")).font(.caption)
                    ScrollView {
                        Text(verbatim: result.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier("assistant-result")
                    }.frame(maxHeight: .infinity)
                } else { Spacer(minLength: 0) }
            } else { Text(BrowserStrings.text("Waiting for the current document…")).foregroundStyle(.secondary); Spacer() }
            Button(BrowserStrings.text("Model settings and permissions…")) { model.openPermissions() }.accessibilityIdentifier("assistant-settings")
        }.padding(14).frame(minWidth: 280, idealWidth: 320, maxWidth: 460, maxHeight: .infinity)
            .accessibilityElement(children: .contain).accessibilityIdentifier("assistant-panel")
    }
}

struct BrowserAssistantCommands: Commands {
    @ObservedObject private var localization = BrowserLocalization.shared
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandMenu(BrowserStrings.text("Assistant")) {
            Button(BrowserStrings.text(model.assistantPresented ? "Hide Assistant" : "Show Assistant")) { model.toggleAssistant() }
                .keyboardShortcut("a", modifiers: [.command, .shift])
            Button(BrowserStrings.text("Summarize Page")) { model.showAndAskAssistant() }.disabled(!model.canOpenAssistantTask)
            Button(BrowserStrings.text("Organize Page")) { model.showAndAskAssistant(organize: true) }.disabled(!model.canOpenAssistantTask)
        }
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import SwiftUI

struct ProfileEditor: Identifiable {
    enum Mode { case create, rename(UInt64), remove(UInt64) }
    let id = UUID()
    let mode: Mode
    let name: String
    var contextID: UInt64? { switch mode { case .create: return nil; case .rename(let id), .remove(let id): return id } }
    var title: String { switch mode { case .create: return "New Profile"; case .rename: return "Rename Profile"; case .remove: return "Remove Profile" } }
}

struct ProfileMenuItems: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject var workspace: BrowserWorkspace
    var body: some View {
        Button("New Profile…") { model.profileEditor = ProfileEditor(mode: .create, name: "") }
            .disabled(workspace.contexts.count >= 16)
        ForEach(workspace.contexts) { context in
            Menu(context.name) {
                Button("Open Window") { Task { await workspace.createWindow(contextID: context.id) } }
                Button("Rename…") { model.profileEditor = ProfileEditor(mode: .rename(context.id), name: context.name) }
                if context.id != 1 {
                    Button("Remove…") { model.profileEditor = ProfileEditor(mode: .remove(context.id), name: context.name) }
                }
            }
        }
        Divider()
        Button("Refresh Profiles") { Task { await workspace.refreshContexts() } }
    }
}

struct BrowserProfileCommands: Commands {
    @ObservedObject var model: BrowserModel
    @ObservedObject var workspace: BrowserWorkspace
    var body: some Commands {
        CommandMenu("Profiles") {
            ProfileMenuItems(model: model, workspace: workspace)
                .disabled(!workspace.canManageContexts || workspace.busy)
        }
    }
}

struct BrowserProfileMenu: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject var workspace: BrowserWorkspace
    var body: some View {
        Menu { ProfileMenuItems(model: model, workspace: workspace) } label: { Image(systemName: "person.crop.circle").frame(width: 24, height: 24) }
            .accessibilityLabel("Profiles").accessibilityValue(model.profileName).accessibilityIdentifier("profile-menu")
            .help(model.profileName).disabled(!workspace.canManageContexts || workspace.busy)
    }
}

struct BrowserProfileEditor: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject var workspace: BrowserWorkspace
    let editor: ProfileEditor
    @State private var name: String
    init(model: BrowserModel, workspace: BrowserWorkspace, editor: ProfileEditor) {
        self.model = model; self.workspace = workspace; self.editor = editor
        _name = State(initialValue: editor.name)
    }
    private var normalized: String { BrowserContextSummary.normalizedName(name) }
    private var valid: Bool {
        BrowserContextSummary.validName(name) && !workspace.contexts.contains { $0.id != editor.contextID && $0.name.lowercased() == normalized.lowercased() }
    }
    private var current: Bool { editor.contextID.map { id in workspace.contexts.contains { $0.id == id } } ?? true }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(editor.title).font(.headline)
            if case .remove(let id) = editor.mode {
                let count = workspace.contexts.first { $0.id == id }?.windows.count ?? 0
                Text("Remove \(editor.name)? This closes its \(count) window(s) and their tabs, and removes its saved profile name.")
            } else {
                TextField("Profile name", text: $name).textFieldStyle(.roundedBorder).accessibilityIdentifier("profile-name")
                if !name.isEmpty && !valid { Text("Enter a shorter, unique profile name.").font(.caption).accessibilityIdentifier("profile-validation") }
            }
            if !current { Text("This profile was removed.").accessibilityIdentifier("profile-stale") }
            if let notice = workspace.notice { Text(notice).font(.caption).accessibilityIdentifier("profile-error") }
            HStack {
                Spacer()
                Button("Cancel") { model.profileEditor = nil }.keyboardShortcut(.cancelAction).accessibilityIdentifier("profile-cancel")
                if case .remove(let id) = editor.mode {
                    Button("Remove Profile", role: .destructive) { Task { if await workspace.closeContext(id) { model.profileEditor = nil } } }
                        .disabled(!current || !workspace.canManageContexts || workspace.busy).accessibilityIdentifier("profile-remove")
                } else {
                    Button("Save") {
                        Task {
                            if let id = editor.contextID {
                                if await workspace.renameContext(id, name: normalized) { model.profileEditor = nil }
                            } else if let id = await workspace.createContext(normalized) {
                                model.profileEditor = nil
                                _ = await workspace.createWindow(contextID: id)
                            }
                        }
                    }.disabled(!valid || !current || !workspace.canManageContexts || workspace.busy)
                        .keyboardShortcut(.defaultAction).accessibilityIdentifier("profile-save")
                }
            }
        }.padding(20).frame(width: 430)
    }
}

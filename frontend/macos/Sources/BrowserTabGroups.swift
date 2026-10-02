// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI

struct TabGroupEditor: Identifiable {
    let id = UUID()
    let groupID: UInt64?
    let name: String
    let color: String
    let tabID: UInt64?
}

private func groupColor(_ hex: String) -> Color {
    guard BrowserTabGroup.validColor(hex), let rgb = UInt32(hex.dropFirst(), radix: 16) else { return .gray }
    return Color(.sRGB, red: Double((rgb >> 16) & 255) / 255,
                 green: Double((rgb >> 8) & 255) / 255, blue: Double(rgb & 255) / 255, opacity: 1)
}

struct BrowserTabStrip: View {
    @ObservedObject var model: BrowserModel
    @ObservedObject private var appearance: BrowserAppearance
    init(model: BrowserModel) { self.model = model; appearance = model.appearance }
    private var ungrouped: [BrowserTab] {
        model.tabs.filter { tab in
            guard let id = tab.groupID else { return true }
            return !model.groups.contains { $0.id == id }
        }
    }
    var body: some View {
        HStack(spacing: 8) {
            ScrollView(.horizontal) {
                HStack(spacing: 4) {
                    ForEach(ungrouped) { tab in tabView(tab) }
                    ForEach(model.groups) { group in
                        HStack(spacing: 4) {
                            let members = model.tabs.filter { $0.groupID == group.id }
                            Button { Task { await model.setTabGroupCollapsed(group.id, collapsed: !group.collapsed) } } label: {
                                HStack(spacing: 4) {
                                    Image(systemName: group.collapsed ? "chevron.right" : "chevron.down").font(.caption2)
                                    Circle().fill(groupColor(group.color)).frame(width: 10, height: 10).accessibilityHidden(true)
                                    Text(group.name).lineLimit(1)
                                    Text("\(members.count)").font(.caption).accessibilityHidden(true)
                                }.padding(6)
                            }
                            .background(groupColor(group.color).opacity(0.15))
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(appearance.resolved.highContrast ? Color.primary : Color.clear, lineWidth: 1).allowsHitTesting(false).accessibilityHidden(true))
                            .accessibilityIdentifier("tab-group-\(group.id)")
                            .accessibilityLabel(group.name)
                            .accessibilityValue("\(group.collapsed ? "Collapsed" : "Expanded"), \(members.count) tabs\(members.contains { $0.id == model.selected } ? ", contains selected tab" : "")")
                            .help(group.name)
                            .disabled(model.groupBusy || !model.groupsAvailable)
                            .contextMenu {
                                Button("Edit Group…") { model.beginTabGroupEditor(group: group.id) }
                                Button(group.collapsed ? "Expand Group" : "Collapse Group") {
                                    Task { await model.setTabGroupCollapsed(group.id, collapsed: !group.collapsed) }
                                }
                                Divider()
                                Button("Ungroup and Remove Group") { Task { await model.removeTabGroup(group.id) } }
                            }
                            if !group.collapsed { ForEach(members) { tab in tabView(tab) } }
                        }
                    }
                }
            }.accessibilityIdentifier("tab-strip").frame(height: 28)
            Button { model.beginTabGroupEditor() } label: {
                Image(systemName: "rectangle.3.group").frame(width: 24, height: 24)
            }
            .accessibilityLabel("New tab group").accessibilityIdentifier("new-tab-group").help("New tab group")
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(appearance.resolved.highContrast ? Color.primary : Color.clear, lineWidth: 1).allowsHitTesting(false).accessibilityHidden(true))
            .disabled(!model.groupsAvailable || model.groupBusy)
            Button { model.action(.values("OpenTab", ["url": .null])) } label: {
                Image(systemName: "plus").frame(width: 24, height: 24)
            }
            .accessibilityLabel("New tab").accessibilityIdentifier("add-tab").help("New tab")
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(appearance.resolved.highContrast ? Color.primary : Color.clear, lineWidth: 1).allowsHitTesting(false).accessibilityHidden(true))
            .keyboardShortcut("t", modifiers: .command)
        }.disabled(!model.ready).padding(.horizontal, 12).padding(.vertical, 8)
    }
    private func tabView(_ tab: BrowserTab) -> some View {
        HStack(spacing: 4) {
            Button { model.select(tab.id) } label: { Text(tab.url ?? "New tab").lineLimit(1).frame(maxWidth: 200) }
                .accessibilityIdentifier("tab-\(tab.id)").accessibilityLabel(tab.url ?? "New tab")
                .accessibilityValue(model.selected == tab.id ? "Selected" : "")
            Button { model.action(.unit("CloseTab"), tab: tab.id) } label: { Image(systemName: "xmark").font(.caption) }
                .accessibilityLabel("Close tab").accessibilityIdentifier("close-tab-\(tab.id)")
        }
        .padding(6)
        .background(model.selected == tab.id ? Color.accentColor.opacity(0.15) : Color.clear)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(model.selected == tab.id && appearance.resolved.highContrast ? Color.primary : Color.clear, lineWidth: 2).allowsHitTesting(false).accessibilityHidden(true))
        .contextMenu {
            if let workspace = model.windowManager { TabWindowMenu(workspace: workspace, tab: tab.id, window: model.windowID) }
            Button("New Tab Group…") { model.beginTabGroupEditor(tab: tab.id) }
                .disabled(!model.groupsAvailable || model.groupBusy)
            Menu("Move to Group") {
                Button("No Group") { Task { await model.setTabGroup(nil, tab: tab.id) } }
                ForEach(model.groups) { group in
                    Button(group.name) { Task { await model.setTabGroup(group.id, tab: tab.id) } }
                }
            }.disabled(!model.groupsAvailable || model.groupBusy)
        }
    }
}

struct BrowserTabGroupEditor: View {
    @ObservedObject var model: BrowserModel
    let editor: TabGroupEditor
    @State private var name: String
    @State private var color: String
    @FocusState private var nameFocused: Bool
    init(model: BrowserModel, editor: TabGroupEditor) {
        self.model = model; self.editor = editor
        _name = State(initialValue: editor.name); _color = State(initialValue: editor.color)
    }
    private let palette: [(String, String)] = [("Blue", "#4477cc"), ("Red", "#cc3344"), ("Green", "#228844"), ("Purple", "#8844cc"), ("Orange", "#d66b00"), ("Gray", "#666666")]
    private var targetExists: Bool {
        if let group = editor.groupID { return model.groups.contains { $0.id == group } }
        if let tab = editor.tabID { return model.tabs.contains { $0.id == tab } }
        return true
    }
    private var valid: Bool { BrowserTabGroup.validName(name) && BrowserTabGroup.validColor(color.trimmingCharacters(in: .whitespacesAndNewlines)) && targetExists }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(editor.groupID == nil ? "New Tab Group" : "Edit Tab Group").font(.headline)
            TextField("Name", text: $name).textFieldStyle(.roundedBorder).focused($nameFocused)
                .accessibilityLabel("Group name").accessibilityIdentifier("group-name").onSubmit(save)
            HStack {
                Circle().fill(groupColor(color)).frame(width: 18, height: 18).accessibilityHidden(true)
                TextField("Hex color", text: $color).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Group color").accessibilityIdentifier("group-color").onSubmit(save)
            }
            HStack(spacing: 10) {
                ForEach(palette, id: \.0) { title, hex in
                    Button { color = hex } label: { Circle().fill(groupColor(hex)).frame(width: 22, height: 22) }
                        .buttonStyle(.plain).accessibilityLabel(title).accessibilityIdentifier("group-color-" + title.lowercased())
                }
            }
            if !targetExists { Text("The tab or group is no longer available.").foregroundStyle(.red).accessibilityIdentifier("group-target-closed") }
            else if let error = model.groupError { Text(error).foregroundStyle(.red).accessibilityIdentifier("group-error") }
            else { Text("Use a name of up to 80 characters and a six-digit hex color. Tabs keep their pages and history.").font(.caption).foregroundStyle(.secondary) }
            HStack {
                Spacer()
                Button("Cancel") { model.groupEditor = nil }.keyboardShortcut(.cancelAction).accessibilityIdentifier("group-cancel")
                    .disabled(model.groupBusy)
                Button(model.groupBusy ? "Saving…" : "Save", action: save).keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("group-save").disabled(!valid || model.groupBusy || !model.ready)
            }
        }.padding(24).frame(width: 420).onAppear { nameFocused = true }
    }
    private func save() {
        guard valid, !model.groupBusy else { return }
        Task {
            let saved: Bool
            if let id = editor.groupID { saved = await model.updateTabGroup(id, name: name, color: color) }
            else { saved = await model.createTabGroup(name: name, color: color, tab: editor.tabID) != nil }
            if saved, model.groupEditor?.id == editor.id { model.groupEditor = nil }
        }
    }
}

struct BrowserTabGroupCommands: Commands {
    @ObservedObject var model: BrowserModel
    var body: some Commands {
        CommandGroup(after: .toolbar) {
            Menu("Tab Groups") {
                Button("New Tab Group…") { model.beginTabGroupEditor() }
                    .keyboardShortcut("g", modifiers: [.command, .option])
                ForEach(model.groups) { group in
                    Menu(group.name) {
                        Button("Edit Group…") { model.beginTabGroupEditor(group: group.id) }
                        Button(group.collapsed ? "Expand Group" : "Collapse Group") {
                            Task { await model.setTabGroupCollapsed(group.id, collapsed: !group.collapsed) }
                        }
                        Button("Ungroup and Remove Group") { Task { await model.removeTabGroup(group.id) } }
                    }
                }
            }.disabled(!model.ready || !model.groupsAvailable || model.groupBusy)
        }
    }
}

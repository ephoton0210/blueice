// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import SwiftUI
import UniformTypeIdentifiers

enum BrowserTabDropTarget: Equatable, Sendable { case tab(UInt64), group(UInt64), ungroupedEnd }

struct BrowserTabPlacement: Equatable, Sendable {
    let tab: UInt64
    let source: UInt64
    let destination: UInt64
    let context: UInt64
    let before: UInt64?
    let group: UInt64?
    let reference: UInt64?

    func valid(windows: [BrowserWindowSummary], contexts: [BrowserContextSummary]) -> Bool {
        guard windows.first(where: { $0.id == source })?.tabs.contains(where: { $0.id == tab }) == true,
              let target = windows.first(where: { $0.id == destination }),
              contexts.first(where: { $0.windows.contains(source) })?.id == context,
              let owner = contexts.first(where: { $0.windows.contains(destination) }), owner.id == context else { return false }
        if let group, !owner.groups.contains(where: { $0.id == group }) { return false }
        if let before, !target.tabs.contains(where: { $0.id == before }) { return false }
        if let reference {
            let members = target.tabs.filter { $0.groupID == group }
            guard let index = members.firstIndex(where: { $0.id == reference }) else { return false }
            if before != reference, members.dropFirst(index + 1).first?.id != before { return false }
        }
        return true
    }

    static func resolve(tab: UInt64, source: UInt64, destination: UInt64, target: BrowserTabDropTarget,
                        after: Bool, windows: [BrowserWindowSummary], contexts: [BrowserContextSummary]) -> BrowserTabPlacement? {
        guard let window = windows.first(where: { $0.id == destination }),
              let context = contexts.first(where: { $0.windows.contains(source) })?.id else { return nil }
        let before: UInt64?, group: UInt64?, reference: UInt64?
        switch target {
        case .tab(let id):
            guard let item = window.tabs.first(where: { $0.id == id }) else { return nil }
            group = item.groupID; reference = id
            let members = window.tabs.filter { $0.groupID == group }
            guard let index = members.firstIndex(where: { $0.id == id }) else { return nil }
            before = after ? members.dropFirst(index + 1).first?.id : id
        case .group(let id):
            group = id; reference = nil
            if let index = window.tabs.lastIndex(where: { $0.groupID == id }) {
                before = window.tabs.dropFirst(index + 1).first(where: { $0.id != tab })?.id
            } else { before = nil }
        case .ungroupedEnd: before = nil; group = nil; reference = nil
        }
        let placement = BrowserTabPlacement(tab: tab, source: source, destination: destination, context: context,
                                           before: before, group: group, reference: reference)
        return placement.valid(windows: windows, contexts: contexts) ? placement : nil
    }
}

@MainActor
final class BrowserTabDragging: ObservableObject {
    static let type = UTType(exportedAs: "cc.blueice.native-tab")
    @Published private(set) var isDragging = false
    private struct Drag {
        let token: UUID
        let owner: ObjectIdentifier
        let tab: UInt64
        let source: UInt64
        let expires: Date
    }
    private var active: Drag?
    private var expiration: Task<Void, Never>?
    private let now: () -> Date
    init(now: @escaping () -> Date = Date.init) { self.now = now }
    func cancel() { active = nil; expiration?.cancel(); expiration = nil; isDragging = false }
    func provider(workspace: BrowserWorkspace, tab: UInt64, source: UInt64) -> NSItemProvider {
        let provider = NSItemProvider()
        guard workspace.canPlaceTabs, !workspace.busy, !workspace.restoringSession,
              workspace.windows.first(where: { $0.id == source })?.tabs.contains(where: { $0.id == tab }) == true else { return provider }
        cancel()
        let token = UUID()
        active = Drag(token: token, owner: ObjectIdentifier(workspace), tab: tab, source: source, expires: now().addingTimeInterval(90))
        isDragging = true
        expiration = Task { [weak self] in
            do { try await Task.sleep(for: .seconds(90)) } catch { return }
            if self?.active?.token == token { self?.cancel() }
        }
        // Publish only an ephemeral token, never the tab URL, page/editor data,
        // a file or a permission proposal. Other processes cannot read it.
        let bytes = Data(token.uuidString.utf8)
        provider.registerDataRepresentation(forTypeIdentifier: Self.type.identifier, visibility: .ownProcess) { completion in
            completion(bytes, nil); return nil
        }
        return provider
    }
    func placement(workspace: BrowserWorkspace, window: UInt64, target: BrowserTabDropTarget, after: Bool) -> BrowserTabPlacement? {
        guard workspace.canPlaceTabs, !workspace.busy, !workspace.restoringSession,
              let active, active.owner == ObjectIdentifier(workspace), active.expires > now() else { return nil }
        return BrowserTabPlacement.resolve(tab: active.tab, source: active.source, destination: window,
            target: target, after: after, windows: workspace.windows, contexts: workspace.contexts)
    }
    func drop(_ providers: [NSItemProvider], workspace: BrowserWorkspace, window: UInt64,
              target: BrowserTabDropTarget, after: Bool) async -> Bool {
        guard providers.count == 1, let provider = providers.first,
              provider.hasItemConformingToTypeIdentifier(Self.type.identifier), let active,
              let intent = placement(workspace: workspace, window: window, target: target, after: after) else { return false }
        let token = active.token
        let bytes: Data?
        do {
            bytes = try await withCheckedThrowingContinuation { continuation in
                provider.loadDataRepresentation(forTypeIdentifier: Self.type.identifier) { data, error in
                    if let error { continuation.resume(throwing: error) }
                    else { continuation.resume(returning: data) }
                }
            }
        } catch { return false }
        guard let bytes, bytes.count == 36, String(data: bytes, encoding: .utf8) == token.uuidString,
              let current = self.active, current.token == token, current.owner == ObjectIdentifier(workspace) else { return false }
        // A live reference may have moved while its provider loaded. Resolve
        // the edge again so an old neighbor cannot place the tab elsewhere.
        guard placement(workspace: workspace, window: window, target: target, after: after) == intent else {
            cancel(); return false
        }
        cancel()
        return await workspace.placeTab(intent)
    }
}

struct BrowserTabDragSource: ViewModifier {
    @ObservedObject var model: BrowserModel
    let tab: UInt64
    @ViewBuilder func body(content: Content) -> some View {
        if let workspace = model.windowManager, workspace.canPlaceTabs {
            content.onDrag { workspace.tabDragging.provider(workspace: workspace, tab: tab, source: model.windowID) }
        } else { content }
    }
}

struct BrowserTabDropArea: ViewModifier {
    @ObservedObject var model: BrowserModel
    let target: BrowserTabDropTarget
    @State private var width: CGFloat = 0
    @State private var edge: Bool?
    @ViewBuilder func body(content: Content) -> some View {
        if let workspace = model.windowManager, workspace.canPlaceTabs {
            content
                .background(GeometryReader { geometry in
                    Color.clear.onAppear { width = geometry.size.width }.onChange(of: geometry.size.width) { _, value in width = value }
                }.allowsHitTesting(false).accessibilityHidden(true))
                .overlay(alignment: edge == true ? .trailing : .leading) {
                    if edge != nil { RoundedRectangle(cornerRadius: 1).fill(Color.accentColor).frame(width: 3).allowsHitTesting(false).accessibilityHidden(true) }
                }
                .onDrop(of: [BrowserTabDragging.type], delegate: BrowserTabDropDelegate(workspace: workspace, window: model.windowID,
                    target: target, width: width, edge: $edge))
                .onReceive(workspace.tabDragging.$isDragging) { dragging in
                    if !dragging, edge != nil { edge = nil }
                }
        } else { content }
    }
}

private struct BrowserTabDropDelegate: DropDelegate {
    let workspace: BrowserWorkspace
    let window: UInt64
    let target: BrowserTabDropTarget
    let width: CGFloat
    @Binding var edge: Bool?
    private func after(_ info: DropInfo) -> Bool { if case .tab = target { return info.location.x >= width / 2 }; return false }
    func validateDrop(info: DropInfo) -> Bool {
        info.hasItemsConforming(to: [BrowserTabDragging.type])
            && workspace.tabDragging.placement(workspace: workspace, window: window, target: target, after: after(info)) != nil
    }
    func dropEntered(info: DropInfo) { if validateDrop(info: info) { edge = after(info) } }
    func dropUpdated(info: DropInfo) -> DropProposal? {
        guard validateDrop(info: info) else { edge = nil; return DropProposal(operation: .cancel) }
        edge = after(info); return DropProposal(operation: .move)
    }
    func dropExited(info: DropInfo) { edge = nil }
    func performDrop(info: DropInfo) -> Bool {
        edge = nil
        let providers = info.itemProviders(for: [BrowserTabDragging.type])
        guard validateDrop(info: info), providers.count == 1 else { return false }
        let after = after(info)
        Task { _ = await workspace.tabDragging.drop(providers, workspace: workspace, window: window, target: target, after: after) }
        return true
    }
}

struct BrowserTabPlacementButtons: View {
    @ObservedObject var workspace: BrowserWorkspace
    let tab: UInt64?
    let window: UInt64
    let shortcuts: Bool
    var body: some View {
        button("Move Tab Left", step: -1, key: .leftArrow)
        button("Move Tab Right", step: 1, key: .rightArrow)
    }
    @ViewBuilder private func button(_ title: String, step: Int, key: KeyEquivalent) -> some View {
        let intent = tab.flatMap { workspace.tabStep($0, in: window, step: step) }
        let button = Button(BrowserStrings.text(title)) { if let intent { Task { _ = await workspace.placeTab(intent) } } }
            .disabled(intent == nil)
        if shortcuts { button.keyboardShortcut(key, modifiers: [.command, .control]) }
        else { button }
    }
}

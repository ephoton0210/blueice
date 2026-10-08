// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest
import UniformTypeIdentifiers

@MainActor
final class FileInputTests: XCTestCase {
    func testNativeActivationWirePreservesGestureContextAndOptionalFileHint() throws {
        let context = TextInputContext(version: 1,frame_source: 7,document_generation: 8,focus_generation: 9)
        let data = try BrowserWire.encode(.nativeActivate(context,21,12,34),tab: 3,request: 21)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: data.dropFirst(4)) as? [String: Any])
        let command = try XCTUnwrap((root["message"] as? [String: Any])?["NativeActivate"] as? [String: Any])
        XCTAssertEqual((command["context"] as? [String: Any])?["document_generation"] as? Int,8)
        XCTAssertEqual(command["gesture"] as? Int,21); XCTAssertEqual(root["request_id"] as? Int,21); XCTAssertEqual(root["tab_id"] as? Int,3)
        let empty = Data("{\"request_id\":21,\"tab_id\":3,\"message\":{\"NativeActivationCompleted\":{\"gesture\":21,\"file_input\":null}}}".utf8)
        guard case .nativeActivationCompleted(let gesture,let hint) = try JSONDecoder().decode(IncomingEnvelope.self,from: empty).message else {
            return XCTFail("Native activation must acknowledge a gesture without a file control")
        }
        XCTAssertEqual(gesture,21); XCTAssertNil(hint)
    }

    func testActualLabelGestureAloneDeliversCorrelatedFileHintAndIgnoresUnsolicitedReplies() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]); model.address = fixture.origin + "/label-activation"; model.navigateAddress()
        let deadline = Date().addingTimeInterval(15)
        while model.representation?.url != fixture.origin + "/label-activation"
            || model.representation?.generation != model.textInputState?.frame_generation
            || model.displayState?.frameGeneration != model.generation || model.textInputBusy {
            guard Date() < deadline else { return XCTFail("Label document did not finish refreshing") }
            try await Task.sleep(for: .milliseconds(20))
        }
        let upload = try XCTUnwrap(model.representation?.nodes.first(where: { $0.name == "Labelled upload" }))
        let input = try XCTUnwrap(model.textInputState)
        let hint = FileInputState(context: .init(tab_id: 1,frame_source: input.frame_source,
            document_generation: input.document_generation,node_id: upload.id,revision: 0),multiple: false,accept: "",names: [])
        var activations: [FileInputState] = []
        model.nativeFileActivation = { activations.append($0) }
        model.apply(IncomingEnvelope(requestID: nil,tabID: 1,message: .nativeActivationCompleted(21,hint)),pixels: nil)
        model.apply(IncomingEnvelope(requestID: 99,tabID: 1,message: .fileInputState(hint)),pixels: nil)
        XCTAssertTrue(activations.isEmpty)
        let page = try XCTUnwrap(model.representation)
        model.queueDocumentClick(x: upload.bounds.x + 40,y: upload.bounds.y - page.scrollY - 20,
            cssSize: try XCTUnwrap(model.cssViewportSize),tab: 1,epoch: model.accessibilityEpoch,document: input.document_generation)
        let activatedDeadline = Date().addingTimeInterval(10)
        while activations.isEmpty {
            guard Date() < activatedDeadline else { return XCTFail("Owned label gesture did not return a file hint") }
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertEqual(activations.count,1); XCTAssertEqual(activations[0].context.node_id,upload.id)
        model.address = "about:credits"; model.navigateAddress()
        model.apply(IncomingEnvelope(requestID: 99,tabID: 1,message: .nativeActivationCompleted(21,hint)),pixels: nil)
        XCTAssertEqual(activations.count,1); XCTAssertFalse(model.fileInputIsCurrent(hint.context,tab: 1))
        XCTAssertEqual(fixture.requests,["/label-activation"])
        await workspace.stop()
    }

    func testSelectedRegularFilesPreserveBytesAndRejectPathsLinksDirectoriesAndBudgets() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bi-file-read-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("中文.bin")
        let bytes = Data([0,255,13,10,7]); try bytes.write(to: file)
        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSince1970: 1234.125)],ofItemAtPath: file.path)
        let selected = try SelectedFile.read([file],multiple: false)
        XCTAssertEqual(selected.count,1); XCTAssertEqual(selected[0].name,"中文.bin"); XCTAssertEqual(Data(selected[0].bytes),bytes)
        XCTAssertEqual(selected[0].last_modified,1234125)
        let link = root.appendingPathComponent("link.bin")
        try FileManager.default.createSymbolicLink(at: link,withDestinationURL: file)
        XCTAssertThrowsError(try SelectedFile.read([link],multiple: false))
        XCTAssertThrowsError(try SelectedFile.read([root],multiple: false))
        XCTAssertThrowsError(try SelectedFile.read([file,file],multiple: false))
        XCTAssertThrowsError(try SelectedFile.read(Array(repeating: file,count: 17),multiple: true))
        let huge = root.appendingPathComponent("huge.bin"); try Data(repeating: 0,count: SelectedFile.maximumBytes + 1).write(to: huge)
        XCTAssertThrowsError(try SelectedFile.read([huge],multiple: false))
        XCTAssertFalse(SelectedFile.validName("../outside")); XCTAssertFalse(SelectedFile.validName("C:\\secret"))
        XCTAssertFalse(SelectedFile.validName("bad\nname")); XCTAssertFalse(SelectedFile.validName(".."))
        let emojiName = "family-👨‍👩‍👧‍👦.bin"
        XCTAssertTrue(SelectedFile.validName(emojiName))
        let emojiFile = root.appendingPathComponent(emojiName); try bytes.write(to: emojiFile)
        XCTAssertEqual(try SelectedFile.read([emojiFile],multiple: false)[0].name,emojiName)
        XCTAssertEqual(BrowserFilePicker.contentTypes(".txt,text/plain,image/*,audio/*,video/*,invalid/unknown").filter { $0 == .image }.count,1)
    }
    func testActualCoreFileSelectionIsScopedAndDoesNotOpenAPickerFromReplies() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]); model.address = fixture.origin + "/file-input"; model.navigateAddress()
        func waitForFileDocument(value: String? = nil) async throws {
            let deadline = Date().addingTimeInterval(15)
            func current() -> Bool {
                guard let page = model.representation, page.url == fixture.origin + "/file-input",
                      page.generation == model.generation, model.textInputState?.frame_generation == page.generation,
                      let file = page.nodes.first(where: { $0.name == "Upload files" }) else { return false }
                return value.map { file.state.value == $0 } ?? true
            }
            while !current() {
                guard Date() < deadline else { XCTFail("File document did not finish refreshing"); return }
                try await Task.sleep(for: .milliseconds(20))
            }
        }
        try await waitForFileDocument()
        let node = try XCTUnwrap(model.representation?.nodes.first(where: { $0.name == "Upload files" }))
        XCTAssertEqual(node.role,.button); XCTAssertTrue(node.state.fileInput)
        let sheets = NSApp.windows.filter { $0.sheetParent != nil }.count
        let prepared = await model.prepareFileInput(node.id)
        let state = try XCTUnwrap(prepared)
        XCTAssertTrue(state.multiple); XCTAssertEqual(state.context.tab_id,1)
        XCTAssertEqual(NSApp.windows.filter { $0.sheetParent != nil }.count,sheets)
        let applied = await model.setFileInput(state.context,files: [SelectedFile(name:"selected.bin",media_type:"application/octet-stream",bytes:[0,255,7])],tab: 1)
        XCTAssertTrue(applied)
        try await waitForFileDocument(value: "selected.bin")
        let next = await model.prepareFileInput(node.id)
        let updated = try XCTUnwrap(next)
        XCTAssertEqual(updated.names,["selected.bin"])
        let stale = await model.setFileInput(state.context,files: [],tab: 1)
        XCTAssertFalse(stale)
        model.fileInputErrorPresented = false
        model.address = "about:credits"; model.navigateAddress()
        XCTAssertFalse(model.fileInputIsCurrent(updated.context,tab: 1))
        XCTAssertEqual(fixture.requests,["/file-input"])
        let staleValidation = await model.validateFileInput(updated.context,tab: 1)
        let staleCancel = await model.cancelFileInput(updated.context,tab: 1)
        XCTAssertFalse(staleValidation); XCTAssertFalse(staleCancel)
        await workspace.stop()
    }
}

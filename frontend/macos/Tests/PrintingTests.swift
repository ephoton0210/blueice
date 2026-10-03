// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit
import XCTest

@MainActor
final class PrintingTests: XCTestCase {
    private func wait(_ condition: @escaping () -> Bool) async {
        let deadline = Date().addingTimeInterval(15)
        while !condition(), Date() < deadline { try? await Task.sleep(for: .milliseconds(20)) }
        XCTAssertTrue(condition())
    }
    func testActualCorePrintMediaPaginationAndAppKitPDFPreserveLiveDocument() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]); model.address = fixture.origin + "/printing"; model.navigateAddress()
        await wait { model.representation?.url == fixture.origin + "/printing" && model.textInputState != nil && model.generation > 0 }
        let before = try XCTUnwrap(model.textInputState)
        let viewport = try XCTUnwrap(model.displayState)
        let history = model.history
        let requests = fixture.requests
        let process = workspace.processID
        let source = PageRepresentation.frameSource(directory: workspace.session.frameDirectory.path)
        let runtime = workspace.session.runtimeDirectory; let generation = before.document_generation
        let printer = try await Task.detached {
            try BrowserPrintSession(runtime: runtime,tab: 1,context: 1,window: 1,source: source,document: generation)
        }.value
        defer { printer.end() }
        let info = BrowserPrintView.info(); info.paperSize = NSSize(width: 300,height: 300)
        let profile = try PrintProfile.from(info)
        let output = try printer.render(profile)
        XCTAssertGreaterThan(output.pages.count,3)
        // Print media changes the real core's blue screen box to red paper ink.
        let image = try XCTUnwrap(output.pages.first?.image)
        let pixels = try XCTUnwrap(image.dataProvider?.data) as Data
        var red = 0; var blue = 0
        for i in stride(from: 0,to: pixels.count,by: 4) {
            if pixels[i] == 255 && pixels[i+1] == 0 && pixels[i+2] == 0 { red += 1 }
            if pixels[i] == 0 && pixels[i+1] == 0 && pixels[i+2] == 255 { blue += 1 }
        }
        XCTAssertGreaterThan(red,1000); XCTAssertEqual(blue,0)
        let root = URL(fileURLWithPath: "/private/tmp/bi-print-test-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root,withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let pdf = root.appendingPathComponent("core-pages.pdf")
        info.jobDisposition = .save; info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = pdf
        let view = BrowserPrintView(session: printer,output: output)
        let operation = NSPrintOperation(view: view,printInfo: info)
        operation.showsPrintPanel = false; operation.showsProgressPanel = false
        XCTAssertTrue(operation.run()); XCTAssertNil(view.failure)
        let document = try XCTUnwrap(CGPDFDocument(pdf as CFURL))
        XCTAssertEqual(document.numberOfPages,view.output.pages.count)
        XCTAssertEqual(try XCTUnwrap(document.page(at: 1)).getBoxRect(.mediaBox).size,NSSize(width: 300,height: 300))
        XCTAssertEqual(try Data(contentsOf: pdf).prefix(5),Data("%PDF-".utf8))
        XCTAssertEqual(model.textInputState?.document_generation,before.document_generation)
        XCTAssertEqual(model.textInputState?.focus_generation,before.focus_generation)
        XCTAssertEqual(model.displayState?.zoom,viewport.zoom)
        XCTAssertEqual(model.displayState?.frameGeneration,viewport.frameGeneration)
        XCTAssertEqual(model.history.back,history.back); XCTAssertEqual(model.history.forward,history.forward)
        XCTAssertEqual(fixture.requests,requests); XCTAssertEqual(workspace.processID,process)
        info.orientation = .landscape; info.scalingFactor = 0.8
        let next = try PrintProfile.from(info)
        XCTAssertNotEqual(next,profile); XCTAssertNoThrow(try printer.render(next))
        let dir = workspace.session.frameDirectory.appendingPathComponent("print-" + (printer.ticket ?? ""))
        printer.end(); XCTAssertFalse(FileManager.default.fileExists(atPath: dir.path))
        await workspace.stop()
    }

    func testReplacedDocumentInvalidatesPreviewAndReleasesItsPixels() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]); model.address = fixture.origin + "/printing"; model.navigateAddress()
        await wait { model.canPrint && model.representation?.url == fixture.origin + "/printing" }
        let document = try XCTUnwrap(model.textInputState?.document_generation)
        let source = PageRepresentation.frameSource(directory: workspace.session.frameDirectory.path)
        let printer = try BrowserPrintSession(runtime: workspace.session.runtimeDirectory,tab: 1,context: 1,window: 1,source: source,document: document)
        defer { printer.end() }
        let output = try printer.render(PrintProfile.from(BrowserPrintView.info()))
        let directory = workspace.session.frameDirectory.appendingPathComponent("print-" + (printer.ticket ?? ""))
        XCTAssertTrue(FileManager.default.fileExists(atPath: directory.path))
        let view = BrowserPrintView(session: printer,output: output)
        model.address = "about:credits"; model.navigateAddress()
        XCTAssertFalse(model.canPrint)
        await wait { model.representation?.url == "about:credits" && model.canPrint }
        XCTAssertThrowsError(try printer.validate())
        var range = NSRange(location: 0,length: 99)
        XCTAssertTrue(view.knowsPageRange(&range)); XCTAssertEqual(range.length,0); XCTAssertNotNil(view.failure)
        XCTAssertEqual(view.rectForPage(1),.zero)
        XCTAssertFalse(FileManager.default.fileExists(atPath: directory.path))
        await workspace.stop()
    }

    func testUnsupportedPrintAreaCancelsThenRecoversBeforePDFDelivery() async throws {
        let fixture = try HTTPFixture(); defer { fixture.stop() }
        let launcher = Bundle(for: Self.self).bundleURL.deletingLastPathComponent().appendingPathComponent("BlueIce.app/Contents/MacOS/blueice-launcher")
        let workspace = BrowserWorkspace(); await workspace.start(launcher: launcher)
        defer { Task { await workspace.stop() } }
        let model = try XCTUnwrap(workspace.models[1]); model.address = fixture.origin + "/printing"; model.navigateAddress()
        await wait { model.canPrint && model.representation?.url == fixture.origin + "/printing" }
        let document = try XCTUnwrap(model.textInputState?.document_generation)
        let source = PageRepresentation.frameSource(directory: workspace.session.frameDirectory.path)
        let printer = try BrowserPrintSession(runtime: workspace.session.runtimeDirectory,tab: 1,context: 1,window: 1,source: source,document: document)
        defer { printer.end() }
        let info = BrowserPrintView.info(); let output = try printer.render(PrintProfile.from(info))
        let view = BrowserPrintView(session: printer,output: output)
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("bi-invalid-print-" + UUID().uuidString + ".pdf")
        defer { try? FileManager.default.removeItem(at: file) }
        info.jobDisposition = .save; info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = file
        let operation = NSPrintOperation(view: view,printInfo: info); operation.showsPrintPanel = false; operation.showsProgressPanel = false
        let previous = NSPrintOperation.current
        defer { NSPrintOperation.current = previous }
        NSPrintOperation.current = operation
        operation.printInfo.scalingFactor = 0.01
        var range = NSRange(location: 0,length: 99)
        XCTAssertTrue(view.knowsPageRange(&range)); XCTAssertEqual(range.length,0); XCTAssertNotNil(view.failure)
        XCTAssertEqual(operation.printInfo.jobDisposition,.cancel)
        // The native panel can recover while the user finishes a numeric value.
        operation.printInfo.scalingFactor = 1; operation.printInfo.jobDisposition = .save
        XCTAssertTrue(view.knowsPageRange(&range)); XCTAssertEqual(range.length,output.pages.count); XCTAssertNil(view.failure)
        NSPrintOperation.current = previous
        XCTAssertTrue(operation.run()); XCTAssertNil(view.failure)
        let pdf = try XCTUnwrap(CGPDFDocument(file as CFURL)); XCTAssertEqual(pdf.numberOfPages,output.pages.count)
        await workspace.stop()
    }

    func testPrintMetadataAndPaperSettingsRejectInvalidOrUnboundedPages() throws {
        let ticket = String(repeating: "a",count: 32)
        let json: [String:Any] = ["Rendered": ["ticket":ticket,"revision":1,"profile":["width_points":225,"height_points":150,"backgrounds":true],"pages":[["shm_path":"/tmp/print.rgba","width":600,"height":400,"height_points":150]]]]
        let valid = try JSONSerialization.data(withJSONObject: json)
        XCTAssertNoThrow(try JSONDecoder().decode(PrintReply.self,from: valid))
        for bad in [String(repeating:"z",count:32),"../outside",""] { XCTAssertFalse(PrintReply.validTicket(bad)) }
        var root = json; var rendered = try XCTUnwrap(root["Rendered"] as? [String:Any]); rendered["revision"] = 0; root["Rendered"] = rendered
        XCTAssertThrowsError(try JSONDecoder().decode(PrintReply.self,from: JSONSerialization.data(withJSONObject: root)))
        let info = BrowserPrintView.info(); info.scalingFactor = 0.01
        XCTAssertThrowsError(try PrintProfile.from(info))
        XCTAssertFalse(PrintProfile(width_points: .infinity,height_points: 100).valid)
    }
}

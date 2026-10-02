// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import CoreGraphics
import Foundation

struct FramePixels: Sendable {
    let data: Data
    let width: Int
    let height: Int
    let generation: UInt64

    static func read(_ notice: FrameNotice, directory: URL) throws -> FramePixels {
        let root = directory.resolvingSymlinksInPath().standardizedFileURL.path + "/"
        let file = URL(fileURLWithPath: notice.path).resolvingSymlinksInPath().standardizedFileURL
        guard file.path.hasPrefix(root) else { throw BrowserFailure.invalid("Frame is outside this browser session.") }
        guard (1...4096).contains(notice.width), (1...4096).contains(notice.height) else {
            throw BrowserFailure.invalid("Unsupported frame dimensions.")
        }
        let size = notice.width * notice.height * 4
        let properties = try FileManager.default.attributesOfItem(atPath: file.path)
        guard properties[.type] as? FileAttributeType == .typeRegular,
              (properties[.size] as? NSNumber)?.intValue == size else {
            throw BrowserFailure.invalid("Frame dimensions do not match its pixel data.")
        }
        let data = try Data(contentsOf: file, options: .mappedIfSafe)
        guard data.count == size else { throw BrowserFailure.invalid("Frame changed while being read.") }
        return FramePixels(data: data, width: notice.width, height: notice.height, generation: notice.generation)
    }

    func image() throws -> CGImage {
        guard let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                                  bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue).union(.byteOrder32Big),
                                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
            throw BrowserFailure.invalid("Could not display core pixels.")
        }
        return image
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import AppKit

struct NativeDragImage: Decodable, Sendable {
    let regions: [PageNode.Bounds]
    let hotspot: NativeDragMouse.Point
    var valid: Bool {
        !regions.isEmpty && regions.count <= 256 && hotspot.valid(65536)
            && regions.allSatisfy { [$0.x,$0.y,$0.width,$0.height].allSatisfy(\.isFinite) && $0.width > 0 && $0.height > 0 }
    }

    @MainActor
    func preview(_ frame: FramePixels,cssSize: CGSize,viewSize: CGSize? = nil) throws -> (image: NSImage,hotspot: CGPoint)? {
        guard valid, cssSize.width > 0, cssSize.height > 0 else { return nil }
        let viewport = CGRect(origin: .zero,size: cssSize)
        let original = regions.map { CGRect(x: $0.x,y: $0.y,width: $0.width,height: $0.height) }
        let originalBounds = original.reduce(CGRect.null) { $0.union($1) }
        let clipped = original.map { $0.intersection(viewport) }.filter { !$0.isNull && !$0.isEmpty }
        let bounds = clipped.reduce(CGRect.null) { $0.union($1) }
        guard !bounds.isNull, bounds.width > 0, bounds.height > 0 else { return nil }
        let view = viewSize ?? cssSize
        guard view.width > 0, view.height > 0 else { return nil }
        let vx = view.width/cssSize.width, vy = view.height/cssSize.height
        let scale = min(1,512/(bounds.width*vx),512/(bounds.height*vy))
        let size = CGSize(width: bounds.width*vx*scale,height: bounds.height*vy*scale)
        let density = min(2,CGFloat(frame.width)/view.width,CGFloat(frame.height)/view.height)
        let width = max(1,Int(ceil(size.width*density))), height = max(1,Int(ceil(size.height*density)))
        let dx = CGFloat(width)/bounds.width, dy = CGFloat(height)/bounds.height
        var pixels = Data(count: width*height*4)
        // Copy only Core-authorized rectangles. Unselected gaps remain
        // transparent, and the resulting image retains no full-frame bytes.
        frame.data.withUnsafeBytes { source in
            pixels.withUnsafeMutableBytes { destination in
                for rect in clipped {
                    let left = max(0,Int(ceil((rect.minX-bounds.minX)*dx-0.5)))
                    let right = min(width,Int(ceil((rect.maxX-bounds.minX)*dx-0.5)))
                    let top = max(0,Int(ceil((rect.minY-bounds.minY)*dy-0.5)))
                    let bottom = min(height,Int(ceil((rect.maxY-bounds.minY)*dy-0.5)))
                    guard left < right, top < bottom else { continue }
                    for y in top..<bottom {
                        let sy = min(frame.height-1,max(0,Int((bounds.minY+(CGFloat(y)+0.5)/dy)*CGFloat(frame.height)/cssSize.height)))
                        for x in left..<right {
                            let sx = min(frame.width-1,max(0,Int((bounds.minX+(CGFloat(x)+0.5)/dx)*CGFloat(frame.width)/cssSize.width)))
                            let from = (sy*frame.width+sx)*4, to = (y*width+x)*4
                            destination.baseAddress!.advanced(by: to).copyMemory(from: source.baseAddress!.advanced(by: from),byteCount: 4)
                        }
                    }
                }
            }
        }
        let result = FramePixels(data: pixels,width: width,height: height,generation: frame.generation)
        let point = CGPoint(x: (hotspot.x-(bounds.minX-originalBounds.minX))*vx*scale,
                            y: (hotspot.y-(bounds.minY-originalBounds.minY))*vy*scale)
        return (NSImage(cgImage: try result.image(),size: size),point)
    }
}

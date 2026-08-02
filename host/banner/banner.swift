import CoreText
import CoreGraphics
import Foundation

let text = "إعمل"
let font = CTFontCreateWithName("GeezaPro-Bold" as CFString, 170, nil)
let attributes: [CFString: Any] = [
    kCTFontAttributeName: font,
    kCTForegroundColorAttributeName: CGColor(gray: 1.0, alpha: 1.0),
]
let attributed = CFAttributedStringCreate(nil, text as CFString, attributes as CFDictionary)!
let line = CTLineCreateWithAttributedString(attributed)
let bounds = CTLineGetBoundsWithOptions(line, [.useGlyphPathBounds])

let padding: CGFloat = 8
let width = Int(ceil(bounds.width + padding * 2))
let height = Int(ceil(bounds.height + padding * 2))
let space = CGColorSpaceCreateDeviceGray()
let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                        bytesPerRow: width, space: space,
                        bitmapInfo: CGImageAlphaInfo.none.rawValue)!
context.setFillColor(CGColor(gray: 0.0, alpha: 1.0))
context.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(height)))
context.textPosition = CGPoint(x: padding - bounds.origin.x, y: padding - bounds.origin.y)
CTLineDraw(line, context)

let data = context.data!.bindMemory(to: UInt8.self, capacity: width * height)
var pgm = "P5\n\(width) \(height)\n255\n".data(using: .ascii)!
// CGContext row 0 is bottom; PGM expects top-first — flip.
for row in 0..<height {
    pgm.append(Data(bytes: data + row * width, count: width))
}
try! pgm.write(to: URL(fileURLWithPath: "banner.pgm"))
print("wrote banner.pgm \(width)x\(height)")

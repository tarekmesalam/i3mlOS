// Render ASCII 32..126 into an 8x16 monochrome cell each, as a PGM strip.
// A terminal font is a grid, so the whole alphabet is one image and the
// packer's job is arithmetic rather than layout.
import CoreText
import CoreGraphics
import Foundation

let cellWidth = 8, cellHeight = 16
let first: UInt8 = 32, last: UInt8 = 126
let count = Int(last - first) + 1

let font = CTFontCreateWithName("Menlo-Regular" as CFString, 14, nil)
let width = cellWidth * count
let context = CGContext(data: nil, width: width, height: cellHeight, bitsPerComponent: 8,
                        bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                        bitmapInfo: CGImageAlphaInfo.none.rawValue)!
context.setFillColor(CGColor(gray: 0, alpha: 1))
context.fill(CGRect(x: 0, y: 0, width: CGFloat(width), height: CGFloat(cellHeight)))

for index in 0..<count {
    let scalar = Unicode.Scalar(first + UInt8(index))
    let text = String(Character(scalar))
    let attributed = CFAttributedStringCreate(nil, text as CFString, [
        kCTFontAttributeName: font,
        kCTForegroundColorAttributeName: CGColor(gray: 1, alpha: 1),
    ] as CFDictionary)!
    let line = CTLineCreateWithAttributedString(attributed)
    context.textPosition = CGPoint(x: CGFloat(index * cellWidth), y: 4)
    CTLineDraw(line, context)
}

let data = context.data!.bindMemory(to: UInt8.self, capacity: width * cellHeight)
var pgm = "P5\n\(width) \(cellHeight)\n255\n".data(using: .ascii)!
for row in 0..<cellHeight {
    pgm.append(Data(bytes: data + row * width, count: width))
}
try! pgm.write(to: URL(fileURLWithPath: "font.pgm"))
print("wrote font.pgm \(width)x\(cellHeight) for \(count) glyphs")

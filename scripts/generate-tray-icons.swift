// Regenerate with: swift scripts/generate-tray-icons.swift
// The macOS artwork contains only black and transparency; AppKit supplies its tint.
import AppKit

let destination = URL(fileURLWithPath: "src-tauri/icons/tray", isDirectory: true)
try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
let paths: [[NSPoint]] = [
    [NSPoint(x: 3, y: 5), NSPoint(x: 14, y: 5)],
    [NSPoint(x: 11, y: 2), NSPoint(x: 14, y: 5), NSPoint(x: 11, y: 8)],
    [NSPoint(x: 15, y: 13), NSPoint(x: 4, y: 13)],
    [NSPoint(x: 7, y: 10), NSPoint(x: 4, y: 13), NSPoint(x: 7, y: 16)],
]
let svg = """
<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18">
  <path d="M3 5H14M11 2L14 5L11 8M15 13H4M7 10L4 13L7 16" fill="none" stroke="#000" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""
try svg.write(to: destination.appendingPathComponent("template.svg"), atomically: true, encoding: .utf8)
for (name, pixels, color) in [("template", 18, false), ("template@2x", 36, false), ("color", 32, true), ("color@2x", 64, true)] {
    var buffer = [UInt8](repeating: 0, count: pixels * pixels * 4)
    let image = buffer.withUnsafeMutableBytes { bytes -> CGImage in
        let context = CGContext(data: bytes.baseAddress, width: pixels, height: pixels,
            bitsPerComponent: 8, bytesPerRow: pixels * 4, space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.setShouldAntialias(true)
        context.translateBy(x: 0, y: CGFloat(pixels))
        let scale = CGFloat(pixels) / 18
        context.scaleBy(x: scale, y: -scale)
        if color {
            context.setFillColor(CGColor(red: 15.0 / 255, green: 143.0 / 255, blue: 104.0 / 255, alpha: 1))
            context.addPath(CGPath(roundedRect: CGRect(x: 0, y: 0, width: 18, height: 18), cornerWidth: 4, cornerHeight: 4, transform: nil))
            context.fillPath()
            context.setStrokeColor(CGColor(gray: 1, alpha: 1))
        } else { context.setStrokeColor(CGColor(gray: 0, alpha: 1)) }
        context.setLineWidth(1.6)
        context.setLineCap(.round)
        context.setLineJoin(.round)
        for points in paths {
            context.move(to: points[0])
            for point in points.dropFirst() { context.addLine(to: point) }
            context.strokePath()
        }
        return context.makeImage()!
    }
    let bitmap = NSBitmapImageRep(cgImage: image)
    try bitmap.representation(using: .png, properties: [:])!.write(to: destination.appendingPathComponent(name + ".png"))
    if pixels == 36 || pixels == 64 {
        for offset in stride(from: 0, to: buffer.count, by: 4) {
            let alpha = Int(buffer[offset + 3])
            if alpha > 0 && alpha < 255 {
                for channel in 0..<3 { buffer[offset + channel] = UInt8(min(255, Int(buffer[offset + channel]) * 255 / alpha)) }
            }
        }
        try Data(buffer).write(to: destination.appendingPathComponent(name + ".rgba"))
    }
    print("Generated \(name) · \(pixels)×\(pixels)")
}

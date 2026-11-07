// Regenerate the checked-in installer background with:
// swift scripts/dmg-background.swift
import AppKit

let width = 640
let height = 400
let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: width * 2,
    pixelsHigh: height * 2, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
    isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
bitmap.size = NSSize(width: width, height: height)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
NSColor(calibratedRed: 0.96, green: 0.97, blue: 0.99, alpha: 1).setFill()
NSRect(x: 0, y: 0, width: width, height: height).fill()

func centered(_ text: String, y: CGFloat, size: CGFloat, weight: NSFont.Weight, color: NSColor) {
    let attributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: size, weight: weight), .foregroundColor: color,
    ]
    let string = text as NSString
    let measured = string.size(withAttributes: attributes)
    string.draw(at: NSPoint(x: (640 - measured.width) / 2, y: y), withAttributes: attributes)
}
centered("Install Flexspeak", y: 325, size: 26, weight: .semibold, color: .black)
centered("Drag Flexspeak into Applications", y: 286, size: 16, weight: .regular, color: .darkGray)
let arrow = NSBezierPath()
arrow.move(to: NSPoint(x: 288, y: 210))
arrow.line(to: NSPoint(x: 350, y: 210))
arrow.move(to: NSPoint(x: 336, y: 224))
arrow.line(to: NSPoint(x: 350, y: 210))
arrow.line(to: NSPoint(x: 336, y: 196))
arrow.lineWidth = 4
arrow.lineCapStyle = .round
arrow.lineJoinStyle = .round
NSColor(calibratedRed: 0.35, green: 0.40, blue: 0.53, alpha: 1).setStroke()
arrow.stroke()
centered("Then open Flexspeak from Applications.", y: 55, size: 13, weight: .regular, color: .darkGray)
NSGraphicsContext.restoreGraphicsState()
let destination = URL(fileURLWithPath: "assets/installer/background.png")
try FileManager.default.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
try bitmap.representation(using: .png, properties: [:])!.write(to: destination)

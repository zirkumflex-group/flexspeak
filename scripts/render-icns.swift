// Regenerates assets/icon/Flexspeak.icns, the 16–1024 px Finder/Dock fallback, from the
// Icon Composer icon. Needs macOS 26+ (actool must read .icon files):
//   scripts/macos-app.sh            # compiles Assets.car into dist/Flexspeak.app
//   swift scripts/render-icns.swift dist/Flexspeak.app /tmp/Flexspeak.iconset
//   iconutil -c icns -o assets/icon/Flexspeak.icns /tmp/Flexspeak.iconset
import AppKit

let app = CommandLine.arguments[1]
let out = CommandLine.arguments[2]
// Read the asset catalog directly; NSWorkspace's icon is cached and may be a placeholder.
guard let icon = Bundle(path: app)?.image(forResource: "Flexspeak") else {
    fatalError("\(app) has no compiled Flexspeak icon in Assets.car")
}
try FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        icon.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels), from: .zero,
            operation: .copy, fraction: 1)
        NSGraphicsContext.restoreGraphicsState()
        let name = scale == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
        try bitmap.representation(using: .png, properties: [:])!
            .write(to: URL(fileURLWithPath: "\(out)/\(name)"))
    }
}

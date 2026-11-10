import SwiftUI

/// The desktop app's Flexspeak dark palette.
enum Palette {
    static let canvas = Color(hex: 0x1C1D20)
    static let chrome = Color(hex: 0x232427)
    static let surface = Color(hex: 0x26272B)
    static let raised = Color(hex: 0x34353A)
    static let text = Color(hex: 0xEBEBEE)
    static let muted = Color(hex: 0xA0A1A8)
    static let faint = Color(hex: 0x75767E)
    static let accent = Color(hex: 0x8EA8FF)
    static let onAccent = Color(hex: 0x0B0F1C)
    static let good = Color(hex: 0x62D39B)
    static let poor = Color(hex: 0xFF6F7D)
}

extension Color {
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}

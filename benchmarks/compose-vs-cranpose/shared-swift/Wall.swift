// The still wall's data, as `perf-data/src/wall.rs` makes it bit for bit: a
// window of tiles that never change, under a clock line whose text changes
// every frame, with a cursor outline that rests on another tile every frame.

import Foundation

enum WallData {
    /// One load tier: the size of its tiles, in whole points.
    struct Tier {
        let tileWidth: CGFloat
        let tileHeight: CGFloat
    }

    /// The tiers, 1 to 20.
    static let tiers = [
        Tier(tileWidth: 64, tileHeight: 40), Tier(tileWidth: 54, tileHeight: 34),
        Tier(tileWidth: 46, tileHeight: 29), Tier(tileWidth: 40, tileHeight: 25),
        Tier(tileWidth: 34, tileHeight: 21), Tier(tileWidth: 30, tileHeight: 18),
        Tier(tileWidth: 26, tileHeight: 16), Tier(tileWidth: 22, tileHeight: 14),
        Tier(tileWidth: 19, tileHeight: 12), Tier(tileWidth: 16, tileHeight: 10),
        Tier(tileWidth: 14, tileHeight: 9), Tier(tileWidth: 12, tileHeight: 8),
        Tier(tileWidth: 10, tileHeight: 7), Tier(tileWidth: 8, tileHeight: 6),
        Tier(tileWidth: 7, tileHeight: 5), Tier(tileWidth: 6, tileHeight: 4),
        Tier(tileWidth: 5, tileHeight: 4), Tier(tileWidth: 5, tileHeight: 3),
        Tier(tileWidth: 4, tileHeight: 3), Tier(tileWidth: 3, tileHeight: 3),
    ]

    static func tier(_ tier: Int) -> Tier { tiers[min(max(tier, 1), tiers.count) - 1] }

    static let gap: CGFloat = 1
    static let clockHeight: CGFloat = 24
    static let clockText: CGFloat = 14
    static let corner: CGFloat = 2
    static let cursorWidth: CGFloat = 2
    static let backgroundRGB: UInt32 = 0x0F1220

    /// The tiles across and down that fill a wall `width` by `height` points.
    static func grid(_ tier: Tier, _ width: CGFloat, _ height: CGFloat) -> (columns: Int, rows: Int) {
        func fit(_ space: CGFloat, _ tile: CGFloat) -> Int { max(1, Int(((space - gap) / (tile + gap)).rounded(.down))) }
        return (fit(width, tier.tileWidth), fit(height, tier.tileHeight))
    }

    /// The wall's grid in the desktop window, under the top bar and the clock line.
    static func windowGrid(_ tier: Tier, _ window: CGSize) -> (columns: Int, rows: Int) {
        grid(tier, window.width, window.height - 56 - clockHeight)
    }

    static func label(_ index: Int) -> String { String(format: "%05d", index) }

    static func value(_ index: Int) -> String {
        let mixed = (UInt32(truncatingIfNeeded: index) &* 2_654_435_761) >> 22
        return String(format: "%03d", mixed % 1000)
    }

    static func fillRGB(_ index: Int) -> UInt32 { PerfData.paletteRGB[index % 8] }

    static func dotRGB(_ index: Int) -> UInt32 { PerfData.chipBackgroundRGB[(index / 8) % 8] }

    static func cursor(_ frame: Int, _ tiles: Int) -> Int { (frame * 37) % max(tiles, 1) }

    static func clock(_ frame: Int) -> String { String(format: "frame %06d", frame) }

    /// Where tile `index` sits on a wall `columns` tiles across.
    static func origin(_ index: Int, _ columns: Int, _ tier: Tier) -> CGPoint {
        CGPoint(x: gap + CGFloat(index % columns) * (tier.tileWidth + gap),
                y: gap + CGFloat(index / columns) * (tier.tileHeight + gap))
    }

    static func labelSize(_ height: CGFloat) -> CGFloat { height * 0.30 }
    static func valueSize(_ height: CGFloat) -> CGFloat { height * 0.26 }
    static func padding(_ height: CGFloat) -> CGFloat { height * 0.06 }
    static func dotSize(_ height: CGFloat) -> CGFloat { height * 0.22 }
}

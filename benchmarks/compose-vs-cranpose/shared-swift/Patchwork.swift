// The patchwork's data, as `perf-data/src/patchwork.rs` makes it bit for bit:
// nine patches of the still wall's tiles, three across and three down. Two
// never change; in the other seven a frame changes a small part and leaves
// the rest as it was.

import Foundation

enum Patchwork {
    static let columns = 3
    static let rows = 3

    /// The patches in the order the screen shows them, row by row.
    enum Patch: Int, CaseIterable {
        case still, charts, recolor, cards, reorder, deep, scroll, paragraph, pile
    }

    /// The step, in tiles or words, of everything that jumps.
    private static let jump = 37

    /// One patch's tiles: `columns` by `rows` of them, `width` by `height` points each.
    struct Tiles {
        let columns: Int
        let rows: Int
        let width: CGFloat
        let height: CGFloat

        /// The tiles of `tier` that fill a patch `patchWidth` by `patchHeight` points.
        init(_ tier: WallData.Tier, _ patchWidth: CGFloat, _ patchHeight: CGFloat) {
            (columns, rows) = WallData.grid(tier, patchWidth, patchHeight)
            width = tier.tileWidth
            height = tier.tileHeight
        }

        var count: Int { columns * rows }
        var inTurn: InTurn { InTurn(columns: columns, rows: rows) }
    }

    /// The items of a patch that change in turn: `columns` by `rows` of them. Each
    /// frame changes the items of one row, the next row down the frame after.
    struct InTurn {
        let columns: Int
        let rows: Int

        /// The changes of item `index` up to frame `frame`.
        func step(_ index: Int, _ frame: Int) -> Int {
            (index / max(columns, 1) + frame) / max(rows, 1)
        }

        /// The row whose items change in frame `frame`, from frame 1 on.
        func rowChanging(_ frame: Int) -> Int {
            (rows - frame % rows) % rows
        }
    }

    static let chartPoints = 512
    static let chartStroke: CGFloat = 1

    /// The height of chart `index` at `point`, as a fraction of the chart.
    static func chartValue(_ index: Int, _ point: Int) -> Float {
        let at = Float(point) * 0.125
        return 0.5 + 0.38 * sinf(at * 0.32 + Float(index) * 0.77) + 0.08 * sinf(at * 1.7 + Float(index))
    }

    static func chartRGB(_ index: Int) -> UInt32 { PerfData.paletteRGB[index % 8] }

    /// The palette entry of the fill of tile `index` of the recolor patch after `step` changes of its row.
    static func recolorIndex(_ index: Int, _ step: Int) -> Int { (index + step) % 8 }

    /// The fill of tile `index` of the recolor patch after `step` changes of its row.
    static func recolorRGB(_ index: Int, _ step: Int) -> UInt32 {
        PerfData.paletteRGB[recolorIndex(index, step)]
    }

    /// The frames one move of the cards patch takes.
    static let cardFrames = 32

    /// A card of the cards patch in a frame: its number and its top in points from the patch's top.
    struct CardPlace {
        let id: Int
        let top: Float
    }

    /// The cards patch: a card is `columns` tiles across and `rows` rows of tiles under a
    /// header, `height` points tall, and `slots` cards rest in the patch.
    struct Cards {
        let columns: Int
        let rows: Int
        let height: Float
        let slots: Int

        /// The cards of a patch `columns` by `rows` tiles of `tileHeight` points, `patchHeight` points tall.
        init(columns: Int, rows: Int, tileHeight: Float, patchHeight: Float) {
            let cardRows = max(rows / 4, 1)
            let gap = Float(WallData.gap)
            self.columns = columns
            self.rows = cardRows
            height = tileHeight + Float(cardRows) * (tileHeight + gap)
            slots = max(Int((patchHeight - gap) / (height + gap)), 1)
        }

        /// The places a frame may show a card at.
        var places: Int { slots + 1 }
        var tiles: Int { columns * rows }

        /// The card at place `place` in frame `frame`, the topmost first; none at the last
        /// place while no card is on its way in.
        func place(_ place: Int, _ frame: Int) -> CardPlace? {
            let moves = frame / cardFrames
            let along = Float(frame % cardFrames) / Float(cardFrames)
            let gap = Float(WallData.gap)
            let pitch = height + gap
            let came = (moves + 1) / 2 + (moves + 1) % 2
            if moves % 2 == 0 {
                let id = came + slots - 1 - place
                return id < 0 ? nil : CardPlace(id: id, top: gap + (Float(place) - 1 + along) * pitch)
            }
            if place >= slots { return nil }
            let other = min(2, slots - 1)
            let reach = Float(other) * pitch * (1 - abs(2 * along - 1))
            let moved: Float = place == 0 ? reach : place == other ? -reach : 0
            return CardPlace(id: came + slots - 1 - place, top: gap + Float(place) * pitch + moved)
        }

        /// The top of card `id` in frame `frame`, while the frame shows it.
        func top(_ id: Int, _ frame: Int) -> Float? {
            for place in 0..<places {
                if let card = self.place(place, frame), card.id == id { return card.top }
            }
            return nil
        }
    }

    static func cardTitle(_ id: Int) -> String { String(format: "Group %05d", id % 100_000) }

    static func cardRGB(_ id: Int) -> UInt32 { PerfData.gradientEndRGB[id % 8] }

    /// The wall tile at place `tile` of card `id`, whose card holds `tiles` tiles.
    static func cardTile(_ id: Int, _ tile: Int, _ tiles: Int) -> Int { (id * tiles + tile) % 100_000 }

    /// The items of the reorder patch for a patch of `tiles` tiles.
    static func reorderCount(_ tiles: Int) -> Int { max(tiles * 2 / 5, 1) }

    /// The first of the two neighbouring places whose items change places in frame `frame`.
    static func reorderSwap(_ frame: Int, _ count: Int) -> Int {
        (frame &* jump) % max(count - 1, 1)
    }

    /// The width of item `item` of the reorder patch, in tiles: 1 to 3.
    static func reorderUnits(_ item: Int) -> Int {
        Int((UInt32(truncatingIfNeeded: item) &* 2_654_435_761) >> 22) % 3 + 1
    }

    /// The width in points of an item `units` tiles wide.
    static func reorderWidth(_ units: Int, _ tileWidth: CGFloat) -> CGFloat {
        CGFloat(units) * tileWidth + CGFloat(units - 1) * WallData.gap
    }

    /// The boxes around the digits of one nest of the deep patch.
    static let depth = 12

    /// The nests or the charts across and down a patch of `columns` by `rows` tiles.
    static func pairs(_ columns: Int, _ rows: Int) -> (columns: Int, rows: Int) {
        (max(columns / 2, 1), max(rows / 2, 1))
    }

    /// The width or height of an item that covers 2 tiles of `tile` points and the gap between them.
    static func double(_ tile: CGFloat) -> CGFloat { 2 * tile + WallData.gap }

    /// The fill of box `level` of nest `index`, the outermost box being level 0.
    static func deepRGB(_ index: Int, _ level: Int) -> UInt32 { PerfData.paletteRGB[(index + level) % 8] }

    /// The three digits inside nest `index` after `step` changes of its row.
    static func deepValue(_ index: Int, _ step: Int) -> String {
        String(format: "%03d", (index * 7 + step) % 1000)
    }

    /// The rows of the scroll patch's list for rows of `columns` tiles.
    static func scrollRows(_ columns: Int) -> Int { 100_000 / max(columns, 1) }

    /// The first row the scroll patch shows in frame `frame`.
    static func scrollFirst(_ frame: Int, _ total: Int, _ visible: Int) -> Int {
        frame % (max(total - visible, 0) + 1)
    }

    /// The words of the paragraph patch for a patch of `tiles` tiles.
    static func paragraphWords(_ tiles: Int) -> Int { tiles * 3 }

    /// The word of the paragraph that shows the frame in frame `frame`.
    static func paragraphChanged(_ frame: Int, _ words: Int) -> Int { (frame &* jump) % max(words, 1) }

    /// The paragraph in frame `frame`: `words` five-digit words, one of which shows the frame.
    static func paragraph(_ frame: Int, _ words: Int) -> String {
        let changed = paragraphChanged(frame, words)
        var text = [UInt8]()
        text.reserveCapacity(words * 6)
        for word in 0..<words {
            if word > 0 { text.append(UInt8(ascii: " ")) }
            var number = word == changed ? frame % 100_000 : word % 100_000
            var digits: (UInt8, UInt8, UInt8, UInt8, UInt8) = (48, 48, 48, 48, 48)
            withUnsafeMutableBytes(of: &digits) { bytes in
                for place in stride(from: 4, through: 0, by: -1) {
                    bytes[place] = UInt8(48 + number % 10)
                    number /= 10
                }
            }
            withUnsafeBytes(of: &digits) { text.append(contentsOf: $0) }
        }
        return String(decoding: text, as: UTF8.self)
    }

    /// The tiles across and down the pile patch for a patch of `columns` by `rows` tiles.
    static func pileGrid(_ columns: Int, _ rows: Int) -> (columns: Int, rows: Int) {
        (max(columns - 1, 1), max(rows - 1, 1))
    }

    /// The opacity of the pile patch's tiles' fills, of 255, and the fill of its opaque tile.
    static let pileAlpha = 0x99
    static let pileOpaqueRGB: UInt32 = 0x000000

    /// The tile of the pile patch whose fill is opaque in frame `frame`.
    static func pileOpaque(_ frame: Int, _ tiles: Int) -> Int { (frame &* jump) % max(tiles, 1) }
}

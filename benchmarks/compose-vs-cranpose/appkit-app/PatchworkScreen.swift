// The patchwork in AppKit: nine patches of the still wall's tiles beside a
// chat. The benchmark's README describes them; `desktop.py` runs it with
// `PERF_SCENARIO=patchwork`.
//
// In AppKit's idiom a screen of many small elements is Core Animation
// layers made once, and a frame sets only the properties that changed, in
// layers that animate nothing. The recolor patch sets the fills of one row of
// tiles, the deep patch the text of one row of nests, the pile patch the fills
// of two tiles. The cards patch moves each card's layer and recycles the
// layer of a card that left. The reorder patch moves the tiles whose places
// changed. The scroll patch moves one container layer and binds the one row
// layer that scrolled out to the row that scrolled in. The paragraph is
// one text layer whose string a frame sets. The charts are shape layers with
// paths built once.

import AppKit
import QuartzCore

/// The colors the patches set, as Core Graphics colors made once.
enum Colors {
    static let palette = PerfData.paletteRGB.map { NSColor(rgb: $0).cgColor }
    static let translucent = PerfData.paletteRGB.map {
        NSColor(rgb: $0, alpha: CGFloat(Patchwork.pileAlpha) / 255).cgColor
    }
    static let dots = PerfData.chipBackgroundRGB.map { NSColor(rgb: $0).cgColor }
    static let gradientEnd = PerfData.gradientEndRGB.map { NSColor(rgb: $0).cgColor }
    static let opaque = NSColor(rgb: Patchwork.pileOpaqueRGB).cgColor
}

/// What every tile of a tier shares: the styles of its texts and where they sit.
struct TileStyles {
    let label: TextStyle
    let value: TextStyle
    let labelSize: CGSize
    let valueSize: CGSize
    /// The three digits of a nest, in the label's style.
    let digitsSize: CGSize
    let padding: CGFloat
    let dot: CGFloat

    init(_ tier: WallData.Tier) {
        let height = tier.tileHeight
        label = TextStyle(WallData.labelSize(height), .white, bold: true)
        value = TextStyle(WallData.valueSize(height), .white)
        padding = WallData.padding(height)
        dot = WallData.dotSize(height)
        // Digits are as wide as one another, so one text of each kind stands for all.
        func measured(_ text: String, _ style: TextStyle) -> CGSize {
            let layer = TextLayer()
            layer.set(text, style)
            return layer.size(within: .greatestFiniteMagnitude)
        }
        labelSize = measured("00000", label)
        valueSize = measured("000", value)
        digitsSize = measured("000", label)
    }
}

/// One tile: its label over a dot and its value, on a fill. Made once; a
/// tile of a recycled layer is bound to another index.
final class Tile {
    let layer: CALayer = stillLayer()
    private let label: TextLayer = stillLayer(TextLayer())
    private let value: TextLayer = stillLayer(TextLayer())
    private let spot: CALayer = stillLayer()
    private let styles: TileStyles

    init(_ index: Int, size: CGSize, _ styles: TileStyles, fill: CGColor) {
        self.styles = styles
        layer.bounds = CGRect(origin: .zero, size: size)
        layer.cornerRadius = WallData.corner
        label.frame = CGRect(origin: CGPoint(x: styles.padding, y: styles.padding), size: styles.labelSize)
        let line = styles.padding + styles.labelSize.height
        value.frame = CGRect(origin: CGPoint(x: styles.padding + styles.dot * 1.5, y: line), size: styles.valueSize)
        spot.frame = CGRect(x: styles.padding, y: line + (styles.valueSize.height - styles.dot) / 2,
                            width: styles.dot, height: styles.dot)
        spot.cornerRadius = styles.dot / 2
        for sublayer in [label, spot, value] { layer.addSublayer(sublayer) }
        bind(index, fill: fill)
    }

    func bind(_ index: Int, fill: CGColor) {
        layer.backgroundColor = fill
        label.set(WallData.label(index), styles.label)
        value.set(WallData.value(index), styles.value)
        spot.backgroundColor = Colors.dots[(index / 8) % Colors.dots.count]
    }

    func move(to origin: CGPoint) {
        layer.frame = CGRect(origin: origin, size: layer.bounds.size)
    }
}

/// Adds the charts, which never change: shape layers whose paths are built once.
func addCharts(to container: CALayer, _ tiles: Patchwork.Tiles) {
    let pairs = Patchwork.pairs(tiles.columns, tiles.rows)
    let size = CGSize(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height))
    for index in 0..<pairs.columns * pairs.rows {
        let chart: CAShapeLayer = stillLayer(CAShapeLayer())
        let path = CGMutablePath()
        let step = size.width / CGFloat(Patchwork.chartPoints - 1)
        for point in 0..<Patchwork.chartPoints {
            let at = CGPoint(x: CGFloat(point) * step,
                             y: size.height * CGFloat(1 - Patchwork.chartValue(index, point)))
            if point == 0 { path.move(to: at) } else { path.addLine(to: at) }
        }
        chart.frame = CGRect(origin: cell(index, pairs.columns, size), size: size)
        chart.path = path
        chart.fillColor = nil
        chart.strokeColor = NSColor(rgb: Patchwork.chartRGB(index)).cgColor
        chart.lineWidth = Patchwork.chartStroke
        container.addSublayer(chart)
    }
}

/// Where cell `index` of a grid `columns` across sits, for cells of `size` a gap apart.
func cell(_ index: Int, _ columns: Int, _ size: CGSize) -> CGPoint {
    CGPoint(x: WallData.gap + CGFloat(index % columns) * (size.width + WallData.gap),
            y: WallData.gap + CGFloat(index / columns) * (size.height + WallData.gap))
}

/// Tiles whose fill changes in turn: a frame sets the fills of one row.
final class RecolorPatch {
    private let rows: [[Tile]]
    private let turn: Patchwork.InTurn

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, _ tier: WallData.Tier, _ styles: TileStyles) {
        let turn = tiles.inTurn
        self.turn = turn
        let size = CGSize(width: tiles.width, height: tiles.height)
        rows = (0..<tiles.rows).map { row in
            (0..<tiles.columns).map { column in
                let index = row * tiles.columns + column
                let tile = Tile(index, size: size, styles,
                                fill: Colors.palette[Patchwork.recolorIndex(index, turn.step(index, 0))])
                tile.move(to: WallData.origin(index, tiles.columns, tier))
                container.addSublayer(tile.layer)
                return tile
            }
        }
    }

    func advance(_ frame: Int) {
        let row = turn.rowChanging(frame)
        let step = turn.step(row * turn.columns, frame)
        for (column, tile) in rows[row].enumerated() {
            tile.layer.backgroundColor = Colors.palette[Patchwork.recolorIndex(row * turn.columns + column, step)]
        }
    }
}

/// A card: a header over rows of tiles, in a layer that moves as a whole.
final class CardLayer {
    let layer: CALayer = stillLayer()
    private let header: CALayer = stillLayer()
    private let title: TextLayer = stillLayer(TextLayer())
    private let tiles: [Tile]
    private let cards: Patchwork.Cards
    private let titleStyle: TextStyle
    private(set) var id = -1
    private(set) var top = CGFloat.nan

    init(_ cards: Patchwork.Cards, _ tiles: Patchwork.Tiles, _ styles: TileStyles) {
        self.cards = cards
        titleStyle = styles.label
        let width = Patchwork.reorderWidth(cards.columns, tiles.width)
        layer.bounds = CGRect(x: 0, y: 0, width: width, height: CGFloat(cards.height))
        header.frame = CGRect(x: 0, y: 0, width: width, height: tiles.height)
        header.cornerRadius = WallData.corner
        title.frame = CGRect(x: styles.padding, y: styles.padding, width: width - 2 * styles.padding,
                             height: styles.labelSize.height)
        header.addSublayer(title)
        layer.addSublayer(header)
        let size = CGSize(width: tiles.width, height: tiles.height)
        let host = layer
        let count = cards.tiles
        self.tiles = (0..<count).map { index in
            let tile = Tile(0, size: size, styles, fill: Colors.palette[0])
            tile.move(to: CGPoint(x: CGFloat(index % cards.columns) * (tiles.width + WallData.gap),
                                  y: tiles.height + WallData.gap + CGFloat(index / cards.columns) * (tiles.height + WallData.gap)))
            host.addSublayer(tile.layer)
            return tile
        }
    }

    /// Shows card `id`.
    func bind(_ id: Int) {
        self.id = id
        header.backgroundColor = Colors.gradientEnd[id % Colors.gradientEnd.count]
        title.set(Patchwork.cardTitle(id), titleStyle)
        for (index, tile) in tiles.enumerated() {
            let wall = Patchwork.cardTile(id, index, cards.tiles)
            tile.bind(wall, fill: Colors.palette[wall % Colors.palette.count])
        }
    }

    func move(toTop top: CGFloat) {
        guard top != self.top else { return }
        self.top = top
        layer.frame = CGRect(origin: CGPoint(x: WallData.gap, y: top), size: layer.bounds.size)
    }
}

/// Cards that move every frame: a frame moves each card's layer. When a move
/// begins, the layer of a card that left shows the card that came in.
final class CardsPatch {
    private let container: CALayer
    private let cards: Patchwork.Cards
    private let tiles: Patchwork.Tiles
    private let styles: TileStyles
    private var stack: [CardLayer] = []
    private var spares: [CardLayer] = []
    private var move = -1

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, patchHeight: CGFloat, _ styles: TileStyles) {
        self.container = container
        self.tiles = tiles
        self.styles = styles
        cards = Patchwork.Cards(columns: tiles.columns, rows: tiles.rows, tileHeight: Float(tiles.height),
                                patchHeight: Float(patchHeight))
        advance(0)
    }

    func advance(_ frame: Int) {
        let moves = frame / Patchwork.cardFrames
        if moves != move {
            move = moves
            let ids = (0..<cards.places).compactMap { cards.place($0, moves * Patchwork.cardFrames)?.id }
            spares += stack.filter { !ids.contains($0.id) }
            var kept = Dictionary(uniqueKeysWithValues: stack.filter { ids.contains($0.id) }.map { ($0.id, $0) })
            stack = ids.map { id in
                if let card = kept.removeValue(forKey: id) { return card }
                let card = spares.popLast() ?? CardLayer(cards, tiles, styles)
                card.bind(id)
                return card
            }
            // Later cards lie over earlier ones.
            container.sublayers = stack.map(\.layer)
        }
        for card in stack {
            // A card no frame shows waits above the patch.
            card.move(toTop: CGFloat(cards.top(card.id, frame) ?? -cards.height))
        }
    }
}

/// A wrapping row of items of 1 to 3 tiles' width, two of which change
/// places every frame: a frame moves the tiles whose places changed.
final class ReorderPatch {
    private let tiles: [Tile]
    private var items: [Int]
    private var widths: [CGFloat]
    private var origins: [CGPoint]
    private let height: CGFloat
    private let width: CGFloat
    private var swap: Int

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, patchWidth: CGFloat, _ styles: TileStyles) {
        let count = Patchwork.reorderCount(tiles.count)
        let sizes = (0..<count).map { Patchwork.reorderWidth(Patchwork.reorderUnits($0), tiles.width) }
        height = tiles.height
        width = patchWidth - 2 * WallData.gap
        items = Array(0..<count)
        widths = sizes
        origins = Array(repeating: CGPoint(x: -1, y: -1), count: count)
        self.tiles = (0..<count).map { item in
            Tile(item, size: CGSize(width: sizes[item], height: tiles.height), styles,
                 fill: Colors.palette[item % Colors.palette.count])
        }
        swap = Patchwork.reorderSwap(0, count)
        if count > 1 { exchange() }
        for tile in self.tiles { container.addSublayer(tile.layer) }
        arrange()
    }

    func advance(_ frame: Int) {
        guard items.count > 1 else { return }
        exchange()
        swap = Patchwork.reorderSwap(frame, items.count)
        exchange()
        arrange()
    }

    private func exchange() {
        items.swapAt(swap, swap + 1)
        widths.swapAt(swap, swap + 1)
    }

    /// Places the items left to right, wrapping at the patch's width, and moves those that lie elsewhere.
    private func arrange() {
        var (x, y) = (CGFloat(0), CGFloat(0))
        for (place, item) in items.enumerated() {
            if x > 0 && x + widths[place] > width {
                y += height + WallData.gap
                x = 0
            }
            let origin = CGPoint(x: WallData.gap + x, y: WallData.gap + y)
            if origins[item] != origin {
                origins[item] = origin
                tiles[item].move(to: origin)
            }
            x += widths[place] + WallData.gap
        }
    }
}

/// A nest of boxes around three digits: one layer inside the other.
final class Nest {
    let root: CALayer = stillLayer()
    let digits: TextLayer = stillLayer(TextLayer())

    init(_ index: Int, size: CGSize, _ styles: TileStyles) {
        var rect = CGRect(origin: .zero, size: size)
        var outer: CALayer?
        for level in 0..<Patchwork.depth {
            let box = outer == nil ? root : stillLayer()
            box.frame = rect
            box.backgroundColor = NSColor(rgb: Patchwork.deepRGB(index, level)).cgColor
            outer?.addSublayer(box)
            outer = box
            rect = CGRect(x: styles.padding, y: styles.padding, width: rect.width - 2 * styles.padding,
                          height: rect.height - 2 * styles.padding)
        }
        outer?.masksToBounds = true
        digits.frame = CGRect(origin: CGPoint(x: styles.padding, y: styles.padding), size: styles.digitsSize)
        outer?.addSublayer(digits)
    }
}

/// Nests of boxes around digits that change in turn: a frame sets the digits of one row.
final class DeepPatch {
    private let rows: [[Nest]]
    private let turn: Patchwork.InTurn
    private let style: TextStyle

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, _ styles: TileStyles) {
        let pairs = Patchwork.pairs(tiles.columns, tiles.rows)
        let turn = Patchwork.InTurn(columns: pairs.columns, rows: pairs.rows)
        self.turn = turn
        style = styles.label
        let size = CGSize(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height))
        rows = (0..<pairs.rows).map { row in
            (0..<pairs.columns).map { column in
                let index = row * pairs.columns + column
                let nest = Nest(index, size: size, styles)
                nest.digits.set(Patchwork.deepValue(index, turn.step(index, 0)), styles.label)
                nest.root.frame = CGRect(origin: cell(index, pairs.columns, size), size: size)
                container.addSublayer(nest.root)
                return nest
            }
        }
    }

    func advance(_ frame: Int) {
        let row = turn.rowChanging(frame)
        let step = turn.step(row * turn.columns, frame)
        for (column, nest) in rows[row].enumerated() {
            nest.digits.set(Patchwork.deepValue(row * turn.columns + column, step), style)
        }
    }
}

/// A list of rows of tiles that scrolls by one row every frame: the rows'
/// layers sit in one container that moves, and the row that scrolled out
/// of view is bound to the row that scrolled in.
final class ScrollPatch {
    private final class Row {
        let layer: CALayer = stillLayer()
        let tiles: [Tile]
        var row = -1

        init(_ tiles: Patchwork.Tiles, _ styles: TileStyles) {
            let size = CGSize(width: tiles.width, height: tiles.height)
            let host = layer
            host.bounds = CGRect(x: 0, y: 0, width: CGFloat(tiles.columns) * (tiles.width + WallData.gap), height: tiles.height)
            self.tiles = (0..<tiles.columns).map { column in
                let tile = Tile(0, size: size, styles, fill: Colors.palette[0])
                tile.move(to: CGPoint(x: CGFloat(column) * (tiles.width + WallData.gap), y: 0))
                host.addSublayer(tile.layer)
                return tile
            }
        }

        func bind(_ row: Int, columns: Int, top: CGFloat) {
            self.row = row
            layer.frame = CGRect(origin: CGPoint(x: WallData.gap, y: top), size: layer.bounds.size)
            for (column, tile) in tiles.enumerated() {
                let index = row * columns + column
                tile.bind(index, fill: Colors.palette[index % Colors.palette.count])
            }
        }
    }

    private let list: CALayer = stillLayer()
    private let slots: [Row]
    private let columns: Int
    private let total: Int
    private let visible: Int
    private let pitch: CGFloat

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, patchHeight: CGFloat, _ styles: TileStyles) {
        columns = tiles.columns
        total = Patchwork.scrollRows(tiles.columns)
        visible = tiles.rows
        pitch = tiles.height + WallData.gap
        // The rows that lie in the patch, the last of them in part.
        slots = (0..<Int(((patchHeight - WallData.gap) / pitch).rounded(.up))).map { _ in Row(tiles, styles) }
        for slot in slots { list.addSublayer(slot.layer) }
        container.addSublayer(list)
        advance(0)
    }

    func advance(_ frame: Int) {
        let first = Patchwork.scrollFirst(frame, total, visible)
        list.frame = CGRect(x: 0, y: -CGFloat(first) * pitch, width: 0, height: 0)
        for row in first..<first + slots.count where row < total {
            let slot = slots[row % slots.count]
            if slot.row != row { slot.bind(row, columns: columns, top: WallData.gap + CGFloat(row) * pitch) }
        }
    }
}

/// One wrapping paragraph in one text layer: a frame sets its string, which
/// the layer lays out and draws again.
final class ParagraphPatch {
    private let layer: TextLayer = stillLayer(TextLayer())
    private let style: TextStyle
    private let words: Int

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, patchWidth: CGFloat) {
        words = Patchwork.paragraphWords(tiles.count)
        style = TextStyle(WallData.labelSize(tiles.height), .white, lines: 2)
        layer.lines = .max
        layer.set(Patchwork.paragraph(0, words), style)
        // The layer is as tall as its text; the patch cuts what lies past its edge.
        let width = patchWidth - 2 * WallData.gap
        layer.frame = CGRect(origin: CGPoint(x: WallData.gap, y: WallData.gap),
                             size: CGSize(width: width, height: layer.size(within: width).height))
        container.addSublayer(layer)
    }

    func advance(_ frame: Int) {
        layer.set(Patchwork.paragraph(frame, words), style)
    }
}

/// Translucent tiles lying over one another: a frame sets the fill of two tiles.
final class PilePatch {
    private let tiles: [Tile]
    private var opaque: Int

    init(_ container: CALayer, _ tiles: Patchwork.Tiles, _ styles: TileStyles) {
        let piled = Patchwork.pileGrid(tiles.columns, tiles.rows)
        let size = CGSize(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height))
        let opaque = Patchwork.pileOpaque(0, piled.columns * piled.rows)
        self.opaque = opaque
        self.tiles = (0..<piled.columns * piled.rows).map { index in
            let tile = Tile(index, size: size, styles,
                            fill: index == opaque ? Colors.opaque : Colors.translucent[index % Colors.translucent.count])
            tile.move(to: CGPoint(x: WallData.gap + CGFloat(index % piled.columns) * (tiles.width + WallData.gap),
                                  y: WallData.gap + CGFloat(index / piled.columns) * (tiles.height + WallData.gap)))
            container.addSublayer(tile.layer)
            return tile
        }
    }

    func advance(_ frame: Int) {
        let next = Patchwork.pileOpaque(frame, tiles.count)
        guard next != opaque else { return }
        tiles[opaque].layer.backgroundColor = Colors.translucent[opaque % Colors.translucent.count]
        tiles[next].layer.backgroundColor = Colors.opaque
        opaque = next
    }
}

/// The patchwork's root: the top bar, the clock line, nine patches and the chat.
final class PatchworkView: RootView {
    private let clockLine: TextLayer = stillLayer(TextLayer())
    private let clockStyle = TextStyle(WallData.clockText, .white)
    private let recolor: RecolorPatch
    private let cards: CardsPatch
    private let reorder: ReorderPatch
    private let deep: DeepPatch
    private let scroll: ScrollPatch
    private let paragraph: ParagraphPatch
    private let pile: PilePatch
    private let chat: ChatView

    init() {
        let tier = WallData.tier(Launch.tier)
        let top = 56 + WallData.clockHeight
        let patchWidth = ((Launch.window.width - ChatData.width) / CGFloat(Patchwork.columns)).rounded(.down)
        let patchHeight = ((Launch.window.height - top) / CGFloat(Patchwork.rows)).rounded(.down)
        let tiles = Patchwork.Tiles(tier, patchWidth, patchHeight)
        let styles = TileStyles(tier)
        let containers: [CALayer] = Patchwork.Patch.allCases.map { patch in
            let container = stillLayer()
            container.masksToBounds = true
            container.frame = CGRect(x: CGFloat(patch.rawValue % Patchwork.columns) * patchWidth,
                                     y: top + CGFloat(patch.rawValue / Patchwork.columns) * patchHeight,
                                     width: patchWidth, height: patchHeight)
            return container
        }
        func container(_ patch: Patchwork.Patch) -> CALayer { containers[patch.rawValue] }
        let wall = WallData.grid(tier, patchWidth, patchHeight)
        for index in 0..<wall.columns * wall.rows {
            let tile = Tile(index, size: CGSize(width: tiles.width, height: tiles.height), styles,
                            fill: Colors.palette[index % Colors.palette.count])
            tile.move(to: WallData.origin(index, wall.columns, tier))
            container(.still).addSublayer(tile.layer)
        }
        addCharts(to: container(.charts), tiles)
        recolor = RecolorPatch(container(.recolor), tiles, tier, styles)
        cards = CardsPatch(container(.cards), tiles, patchHeight: patchHeight, styles)
        reorder = ReorderPatch(container(.reorder), tiles, patchWidth: patchWidth, styles)
        deep = DeepPatch(container(.deep), tiles, styles)
        scroll = ScrollPatch(container(.scroll), tiles, patchHeight: patchHeight, styles)
        paragraph = ParagraphPatch(container(.paragraph), tiles, patchWidth: patchWidth)
        pile = PilePatch(container(.pile), tiles, styles)
        chat = ChatView(height: Launch.window.height - top)
        super.init(title: "Patchwork", background: NSColor(rgb: WallData.backgroundRGB))
        for container in containers { layer?.addSublayer(container) }
        layer?.addSublayer(clockLine)
        chat.frame = CGRect(x: Launch.window.width - ChatData.width, y: top, width: ChatData.width,
                            height: Launch.window.height - top)
        addSubview(chat)
        // The digits of the clock are as wide as one another: its layer is placed once.
        clockLine.set(WallData.clock(0), clockStyle)
        let size = clockLine.size(within: .greatestFiniteMagnitude)
        clockLine.place(CGRect(x: 8, y: 56 + (WallData.clockHeight - size.height) / 2, width: size.width,
                               height: size.height))
    }

    required init?(coder: NSCoder) { nil }

    /// Sets what changed in the frame, in layers that animate nothing.
    override func advance(_ frame: Int) {
        clockLine.set(WallData.clock(frame), clockStyle)
        recolor.advance(frame)
        cards.advance(frame)
        reorder.advance(frame)
        deep.advance(frame)
        scroll.advance(frame)
        paragraph.advance(frame)
        pile.advance(frame)
        chat.advance(frame)
    }
}

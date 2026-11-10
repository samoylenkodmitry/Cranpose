// The patchwork in SwiftUI: nine patches of the still wall's tiles beside a
// chat. The benchmark's README describes them; `desktop.py` runs it with
// `PERF_SCENARIO=patchwork`.
//
// In SwiftUI's idiom a view's body runs again only when a value it read
// changes, and a view whose inputs are equal is skipped. Each patch keeps
// its state in small `@Observable` objects that the frame loop moves, and
// the leaf views read only the object of their own row, card or tile, so a
// frame evaluates the one row, the cards' offsets, the two tiles or the one
// text that changed. The tiles' texts are views of their own that a new fill
// never evaluates again. The charts keep their paths built once. The cards
// move with `.offset`, which lays nothing out again. The scroll patch is a
// `LazyVStack` in a `ScrollView` moved by a `ScrollPosition`.

import Observation
import SwiftUI

/// A number that one row, tile or word of a patch reads: no other view
/// evaluates again when it changes.
@Observable
final class Counter {
    var value: Int

    init(_ value: Int) { self.value = value }
}

/// A card of the cards patch: its number, and its top, which only the card's
/// own offset reads.
@Observable
final class CardState: Identifiable {
    let id: Int
    var top: CGFloat

    init(id: Int, top: CGFloat) {
        self.id = id
        self.top = top
    }
}

/// The cards the patch shows; the stack reads them, and a card's own state
/// moves it.
@Observable
final class CardsModel {
    let cards: Patchwork.Cards
    private(set) var stack: [CardState] = []
    @ObservationIgnored private var move = -1

    init(_ cards: Patchwork.Cards) {
        self.cards = cards
        advance(0)
    }

    func advance(_ frame: Int) {
        let moves = frame / Patchwork.cardFrames
        if moves != move {
            // The stack holds the cards of the move's first frame.
            move = moves
            let old = Dictionary(uniqueKeysWithValues: stack.map { ($0.id, $0) })
            stack = (0..<cards.places).compactMap { cards.place($0, moves * Patchwork.cardFrames) }.map {
                old[$0.id] ?? CardState(id: $0.id, top: 0)
            }
        }
        for card in stack {
            // A card no frame shows waits above the patch.
            let top = CGFloat(cards.top(card.id, frame) ?? -cards.height)
            if card.top != top { card.top = top }
        }
    }
}

/// The reorder patch's items in their places, and their widths in points.
@Observable
final class ReorderModel {
    private(set) var items: [Int]
    private(set) var widths: [CGFloat]
    @ObservationIgnored private var swap: Int

    init(count: Int, tileWidth: CGFloat) {
        items = Array(0..<count)
        widths = (0..<count).map { Patchwork.reorderWidth(Patchwork.reorderUnits($0), tileWidth) }
        swap = Patchwork.reorderSwap(0, count)
        if count > 1 { exchange() }
    }

    /// Puts the two items of the previous frame back and swaps the two of this frame.
    func advance(_ frame: Int) {
        guard items.count > 1 else { return }
        exchange()
        swap = Patchwork.reorderSwap(frame, items.count)
        exchange()
    }

    private func exchange() {
        items.swapAt(swap, swap + 1)
        widths.swapAt(swap, swap + 1)
    }
}

/// The paragraph's text.
@Observable
final class ParagraphModel {
    private(set) var text: String
    @ObservationIgnored private let words: Int

    init(words: Int) {
        self.words = words
        text = Patchwork.paragraph(0, words)
    }

    func advance(_ frame: Int) { text = Patchwork.paragraph(frame, words) }
}

/// The offset the scroll patch's list takes in each frame.
@Observable
final class ScrollModel {
    private(set) var offset: CGFloat = 0
    @ObservationIgnored private let rows: Int
    @ObservationIgnored private let visible: Int
    @ObservationIgnored private let pitch: CGFloat

    init(tiles: Patchwork.Tiles) {
        rows = Patchwork.scrollRows(tiles.columns)
        visible = tiles.rows
        pitch = tiles.height + WallData.gap
    }

    func advance(_ frame: Int) {
        offset = CGFloat(Patchwork.scrollFirst(frame, rows, visible)) * pitch
    }
}

/// What the patchwork shows, made once; each frame moves the part that changes.
final class PatchworkModel {
    let tier = WallData.tier(Launch.tier)
    let patchWidth = ((Launch.window.width - ChatData.width) / CGFloat(Patchwork.columns)).rounded(.down)
    let patchHeight = ((Launch.window.height - 56 - WallData.clockHeight) / CGFloat(Patchwork.rows)).rounded(.down)
    let tiles: Patchwork.Tiles
    let nests: Patchwork.InTurn
    let recolor: [Counter]
    let deep: [Counter]
    let pile: [Counter]
    let cards: CardsModel
    let reorder: ReorderModel
    let paragraph: ParagraphModel
    let scroll: ScrollModel
    let chat = ChatModel()

    init() {
        tiles = Patchwork.Tiles(tier, patchWidth, patchHeight)
        let pairs = Patchwork.pairs(tiles.columns, tiles.rows)
        nests = Patchwork.InTurn(columns: pairs.columns, rows: pairs.rows)
        recolor = (0..<tiles.rows).map { _ in Counter(0) }
        deep = (0..<nests.rows).map { _ in Counter(0) }
        let piled = Patchwork.pileGrid(tiles.columns, tiles.rows)
        pile = (0..<piled.columns * piled.rows).map { Counter($0 == Patchwork.pileOpaque(0, piled.columns * piled.rows) ? 1 : 0) }
        cards = CardsModel(Patchwork.Cards(columns: tiles.columns, rows: tiles.rows, tileHeight: Float(tiles.height),
                                           patchHeight: Float(patchHeight)))
        reorder = ReorderModel(count: Patchwork.reorderCount(tiles.count), tileWidth: tiles.width)
        paragraph = ParagraphModel(words: Patchwork.paragraphWords(tiles.count))
        scroll = ScrollModel(tiles: tiles)
    }

    /// Moves everything to frame `frame`, which follows the one before.
    func advance(_ frame: Int) {
        let turn = tiles.inTurn
        let row = turn.rowChanging(frame)
        recolor[row].value = turn.step(row * turn.columns, frame)
        let nest = nests.rowChanging(frame)
        deep[nest].value = nests.step(nest * nests.columns, frame)
        let opaque = Patchwork.pileOpaque(frame, pile.count)
        let before = Patchwork.pileOpaque(frame - 1, pile.count)
        if opaque != before {
            pile[before].value = 0
            pile[opaque].value = 1
        }
        cards.advance(frame)
        reorder.advance(frame)
        paragraph.advance(frame)
        scroll.advance(frame)
        chat.advance(frame)
    }
}

struct PatchworkView: View {
    let model: PatchworkModel
    let clock: Clock

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            TopBar(title: "Patchwork")
            WallClock(clock: clock)
            HStack(alignment: .top, spacing: 0) {
                VStack(spacing: 0) {
                    ForEach(0..<Patchwork.rows, id: \.self) { row in
                        HStack(spacing: 0) {
                            ForEach(0..<Patchwork.columns, id: \.self) { column in
                                PatchView(patch: Patchwork.Patch.allCases[row * Patchwork.columns + column], model: model)
                                    .frame(width: model.patchWidth, height: model.patchHeight, alignment: .topLeading)
                                    .clipped()
                            }
                        }
                    }
                }
                ChatView(chat: model.chat, clock: clock)
            }
        }
        .frame(width: Launch.window.width, height: Launch.window.height, alignment: .topLeading)
        .background(Color(rgb: WallData.backgroundRGB))
    }
}

struct PatchView: View {
    let patch: Patchwork.Patch
    let model: PatchworkModel

    var body: some View {
        let tiles = model.tiles
        switch patch {
        case .still:
            WallTiles(columns: tiles.columns, rows: tiles.rows, tier: model.tier)
        case .charts: ChartsPatch(tiles: tiles)
        case .recolor: RecolorPatch(model: model)
        case .cards: CardStack(model: model.cards, tiles: tiles, tier: model.tier)
        case .reorder: ReorderPatch(model: model.reorder, tiles: tiles, tier: model.tier)
        case .deep: DeepPatch(model: model)
        case .scroll: ScrollPatch(model: model.scroll, tiles: tiles, tier: model.tier)
        case .paragraph: ParagraphPatch(model: model.paragraph, tiles: tiles)
        case .pile: PilePatch(model: model)
        }
    }
}

/// Line charts that never change: each keeps the path it built once.
struct ChartsPatch: View {
    let tiles: Patchwork.Tiles

    var body: some View {
        let pairs = Patchwork.pairs(tiles.columns, tiles.rows)
        let size = CGSize(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height))
        Cells(columns: pairs.columns, rows: pairs.rows) { ChartLine(index: $0, size: size) }
    }
}

struct ChartLine: View {
    let index: Int
    let size: CGSize
    private let line: Path

    init(index: Int, size: CGSize) {
        self.index = index
        self.size = size
        var line = Path()
        let step = size.width / CGFloat(Patchwork.chartPoints - 1)
        for point in 0..<Patchwork.chartPoints {
            let at = CGPoint(x: CGFloat(point) * step,
                             y: size.height * CGFloat(1 - Patchwork.chartValue(index, point)))
            if point == 0 { line.move(to: at) } else { line.addLine(to: at) }
        }
        self.line = line
    }

    var body: some View {
        line.stroke(Color(rgb: Patchwork.chartRGB(index)), lineWidth: Patchwork.chartStroke)
            .frame(width: size.width, height: size.height)
    }
}

/// Tiles whose fill changes in turn: a tile reads the step of its row.
struct RecolorPatch: View {
    let model: PatchworkModel

    var body: some View {
        let tiles = model.tiles
        Cells(columns: tiles.columns, rows: tiles.rows) {
            RecolorTile(index: $0, tier: model.tier, row: model.recolor[$0 / tiles.columns])
        }
    }
}

struct RecolorTile: View {
    let index: Int
    let tier: WallData.Tier
    let row: Counter

    var body: some View {
        WallTile(index: index, tier: tier, fill: Color(rgb: Patchwork.recolorRGB(index, row.value)))
    }
}

/// Cards that move every frame: the stack changes when a move begins, and a
/// frame only offsets each card.
struct CardStack: View {
    let model: CardsModel
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier

    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(model.stack) { card in
                CardSlot(state: card, cards: model.cards, tiles: tiles, tier: tier)
            }
        }
    }
}

/// A card at its offset; the card's content is not evaluated again for it.
struct CardSlot: View {
    let state: CardState
    let cards: Patchwork.Cards
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier

    var body: some View {
        CardContent(id: state.id, cards: cards, tiles: tiles, tier: tier).offset(y: state.top)
    }
}

struct CardContent: View {
    let id: Int
    let cards: Patchwork.Cards
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier

    var body: some View {
        let height = tiles.height
        VStack(alignment: .leading, spacing: WallData.gap) {
            Text(Patchwork.cardTitle(id)).font(roboto(WallData.labelSize(height), 1, bold: true)).lineLimit(1)
                .foregroundStyle(.white)
                .padding(WallData.padding(height))
                .frame(width: Patchwork.reorderWidth(cards.columns, tiles.width), height: height, alignment: .topLeading)
                .background(Color(rgb: Patchwork.cardRGB(id)), in: RoundedRectangle(cornerRadius: WallData.corner))
                .clipped()
            ForEach(0..<cards.rows, id: \.self) { row in
                HStack(spacing: WallData.gap) {
                    ForEach(0..<cards.columns, id: \.self) { column in
                        WallTile(index: Patchwork.cardTile(id, row * cards.columns + column, cards.tiles), tier: tier)
                    }
                }
            }
        }
        .padding(.horizontal, WallData.gap)
    }
}

/// A wrapping row of items, two of which change places every frame: each
/// keeps its identity, so a frame moves those two and builds none again.
struct ReorderPatch: View {
    let model: ReorderModel
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier

    var body: some View {
        TileFlow(widths: model.widths, height: tiles.height) {
            ForEach(Array(zip(model.items, model.widths)), id: \.0) { item, width in
                WallTile(index: item, tier: tier, size: CGSize(width: width, height: tiles.height))
            }
        }
        .padding(WallData.gap)
    }
}

/// Tiles of known widths left to right, wrapping at the width, `WallData.gap`
/// apart both ways. The layout never asks a tile for its size, and it
/// arranges them once for the size query and the placing together.
struct TileFlow: Layout {
    let widths: [CGFloat]
    let height: CGFloat

    struct Arrangement {
        var width = CGFloat.nan
        var points: [CGPoint] = []
        var bottom: CGFloat = 0
    }

    func makeCache(subviews: Subviews) -> Arrangement { Arrangement() }

    private func arrange(_ width: CGFloat, _ cache: inout Arrangement) {
        guard cache.width != width else { return }
        cache.width = width
        cache.points.removeAll(keepingCapacity: true)
        var (x, y) = (CGFloat(0), CGFloat(0))
        for tile in widths {
            if x > 0 && x + tile > width {
                y += height + WallData.gap
                x = 0
            }
            cache.points.append(CGPoint(x: x, y: y))
            x += tile + WallData.gap
        }
        cache.bottom = y + height
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout Arrangement) -> CGSize {
        let width = proposal.width ?? .infinity
        arrange(width, &cache)
        return CGSize(width: width.isFinite ? width : 0, height: cache.bottom)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout Arrangement) {
        arrange(bounds.width, &cache)
        for (index, subview) in subviews.enumerated() {
            subview.place(at: CGPoint(x: bounds.minX + cache.points[index].x, y: bounds.minY + cache.points[index].y),
                          proposal: ProposedViewSize(width: widths[index], height: height))
        }
    }
}

/// Nests of boxes around digits that change in turn.
struct DeepPatch: View {
    let model: PatchworkModel

    var body: some View {
        let tiles = model.tiles
        let nests = model.nests
        Cells(columns: nests.columns, rows: nests.rows) { index in
            NestBox(index: index, level: 0, height: tiles.height, row: model.deep[index / nests.columns])
                .frame(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height))
        }
    }
}

struct NestBox: View {
    let index: Int
    let level: Int
    let height: CGFloat
    let row: Counter

    var body: some View {
        Group {
            if level + 1 < Patchwork.depth {
                AnyView(NestBox(index: index, level: level + 1, height: height, row: row))
            } else {
                NestDigits(index: index, height: height, row: row)
            }
        }
        .padding(WallData.padding(height))
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(rgb: Patchwork.deepRGB(index, level)))
    }
}

/// The digits inside a nest, the one view of it that reads its row's step.
struct NestDigits: View {
    let index: Int
    let height: CGFloat
    let row: Counter

    var body: some View {
        Text(Patchwork.deepValue(index, row.value)).font(roboto(WallData.labelSize(height), 1, bold: true))
            .foregroundStyle(.white).lineLimit(1).fixedSize()
    }
}

/// A lazy list of rows of tiles, which each frame scrolls by one row.
struct ScrollPatch: View {
    let model: ScrollModel
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier
    @State private var position = ScrollPosition(y: 0)

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            LazyVStack(alignment: .leading, spacing: WallData.gap) {
                ForEach(0..<Patchwork.scrollRows(tiles.columns), id: \.self) { row in
                    ScrollRow(row: row, columns: tiles.columns, tier: tier)
                }
            }
            .padding(WallData.gap)
        }
        .scrollPosition($position)
        .background { ScrollDriver(model: model, position: $position) }
    }
}

/// Moves the list when the offset changes: the view that reads the offset
/// is this empty one, so the list's own body does not evaluate again for it.
struct ScrollDriver: View {
    let model: ScrollModel
    @Binding var position: ScrollPosition

    var body: some View {
        Color.clear.onChange(of: model.offset) { _, offset in position.scrollTo(y: offset) }
    }
}

struct ScrollRow: View {
    let row: Int
    let columns: Int
    let tier: WallData.Tier

    var body: some View {
        HStack(spacing: WallData.gap) {
            ForEach(0..<columns, id: \.self) { column in WallTile(index: row * columns + column, tier: tier) }
        }
    }
}

/// One paragraph, one word of which changes every frame.
struct ParagraphPatch: View {
    let model: ParagraphModel
    let tiles: Patchwork.Tiles

    var body: some View {
        let size = WallData.labelSize(tiles.height)
        Text(verbatim: model.text).font(roboto(size, 1)).lineHeight(size).foregroundStyle(.white)
            .padding(WallData.gap)
    }
}

extension View {
    /// Lines 1.4 em apart, for a face whose own line height is `natural` em: Roboto's is 1.172.
    func lineHeight(_ size: CGFloat, natural: CGFloat = 1.172) -> some View {
        lineSpacing(size * (1.4 - natural))
    }
}

/// Translucent tiles lying over one another: a tile reads only whether it is
/// the opaque one.
struct PilePatch: View {
    let model: PatchworkModel

    var body: some View {
        let tiles = model.tiles
        let piled = Patchwork.pileGrid(tiles.columns, tiles.rows)
        ZStack(alignment: .topLeading) {
            ForEach(0..<piled.columns * piled.rows, id: \.self) { index in
                PileTile(index: index, tiles: tiles, tier: model.tier, columns: piled.columns, opaque: model.pile[index])
            }
        }
    }
}

struct PileTile: View {
    let index: Int
    let tiles: Patchwork.Tiles
    let tier: WallData.Tier
    let columns: Int
    let opaque: Counter

    var body: some View {
        let fill = opaque.value == 1
            ? Color(rgb: Patchwork.pileOpaqueRGB)
            : Color(rgb: WallData.fillRGB(index)).opacity(Double(Patchwork.pileAlpha) / 255)
        WallTile(index: index, tier: tier,
                 size: CGSize(width: Patchwork.double(tiles.width), height: Patchwork.double(tiles.height)), fill: fill)
            .offset(x: WallData.gap + CGFloat(index % columns) * (tiles.width + WallData.gap),
                    y: WallData.gap + CGFloat(index / columns) * (tiles.height + WallData.gap))
    }
}

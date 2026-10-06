// The gauntlet in AppKit: the screen `compose-app/.../Gauntlet.kt` draws,
// element for element, in AppKit's own idiom for many small elements: a view
// for each list row, card and cluster, and Core Animation layers inside them
// for everything a card or a cluster shows. Views are laid out by hand in one
// pass from the window's root; a text layer draws its text once and draws it
// again only when the text changes or a new width wraps or cuts it
// differently; bars, avatars, shadows, sparklines and the tilting badge are
// layer properties and shape paths, which the GPU composites. The list keeps
// only the rows on screen and recycles the rows that scroll off, as
// NSTableView does. The benchmark's README describes it; `desktop.py` runs it
// with the tier in `PERF_TIER` and the Roboto files in `PERF_FONTS`.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. The window's display link advances it once per frame of the
// display and asks the root for a layout; the layout pass sets every value of
// the frame and places every view and layer.

import AppKit
import CoreText
import QuartzCore

/// What `desktop.py` asked of the gauntlet.
enum Launch {
    static let environment = ProcessInfo.processInfo.environment
    static let tier = Int(environment["PERF_TIER"] ?? "") ?? 5
    static let freeze = Int(environment["PERF_FREEZE"] ?? "") ?? 0
    static let fonts = environment["PERF_FONTS"] ?? "fonts"
    /// The window every desktop app opens for the gauntlet, in points.
    static let window = CGSize(width: 1280, height: 820)
}

/// Writes a `PERF` line where `desktop.py` reads it.
func log(_ line: String) {
    print(line)
    fflush(stdout)
}

/// The frame index every per-frame value follows.
enum Clock {
    static var frame = 0
}

extension NSColor {
    convenience init(rgb: UInt32, alpha: CGFloat = 1) {
        self.init(srgbRed: CGFloat(rgb >> 16 & 0xFF) / 255, green: CGFloat(rgb >> 8 & 0xFF) / 255,
                  blue: CGFloat(rgb & 0xFF) / 255, alpha: alpha)
    }

    static let ink = NSColor(rgb: 0x111827)
    static let body = NSColor(rgb: 0x374151)
    static let muted = NSColor(rgb: 0x6B7280)
    static let hairline = NSColor(rgb: 0xE5E7EB)
    static let panel = NSColor(rgb: 0xE2E8F0)
    static let up = NSColor(rgb: 0x16A34A)
    static let down = NSColor(rgb: 0xDC2626)
    static let levelBackground = [NSColor(rgb: 0xF1F5F9), NSColor(rgb: 0xCBD5E1)]
    static let palette = PerfData.paletteRGB.map { NSColor(rgb: $0) }
    static let chipBackground = PerfData.chipBackgroundRGB.map { NSColor(rgb: $0) }
    static let gradientEnd = PerfData.gradientEndRGB.map { NSColor(rgb: $0) }
}

/// Layer properties a value changes without an animation: the values follow
/// the frame, not a transition.
let still: [String: CAAction] = [
    "position": NSNull(), "bounds": NSNull(), "frame": NSNull(), "cornerRadius": NSNull(),
    "backgroundColor": NSNull(), "contents": NSNull(), "path": NSNull(), "transform": NSNull(),
    "colors": NSNull(), "strokeColor": NSNull(), "hidden": NSNull(), "shadowPath": NSNull(),
    "sublayers": NSNull(), "onOrderIn": NSNull(), "onOrderOut": NSNull(),
]

/// The scale the window's pixels have: every layer draws at it.
var backingScale: CGFloat = 2

/// `made`, set to animate nothing and to draw at the window's scale.
func stillLayer<T: CALayer>(_ made: T) -> T {
    made.actions = still
    made.contentsScale = backingScale
    return made
}

/// A plain layer that animates nothing.
func stillLayer() -> CALayer {
    stillLayer(CALayer())
}

/// Roboto in one size, weight and color, with lines 1.4 em apart and the
/// glyphs centered in each line, as Compose centers its line height. Made
/// once per style, so a text layer knows its style by identity.
final class TextStyle {
    let attributes: [NSAttributedString.Key: Any]
    let lineHeight: CGFloat
    /// The font's own height, ascent and descent: a text's first and last
    /// line keep no leading, as Compose trims it.
    let natural: CGFloat

    init(_ size: CGFloat, _ color: NSColor, bold: Bool = false, lines: Int = 1) {
        let font = NSFont(name: bold ? "Roboto-Bold" : "Roboto-Regular", size: size)
            ?? .systemFont(ofSize: size, weight: bold ? .bold : .regular)
        lineHeight = size * 1.4
        natural = font.ascender - font.descender
        let paragraph = NSMutableParagraphStyle()
        paragraph.minimumLineHeight = lineHeight
        paragraph.maximumLineHeight = lineHeight
        paragraph.lineBreakMode = lines == 1 ? .byTruncatingTail : .byWordWrapping
        attributes = [.font: font, .foregroundColor: color, .paragraphStyle: paragraph,
                      .baselineOffset: (lineHeight - natural) / 2]
    }

    func with(_ key: NSAttributedString.Key, _ value: Any) -> [NSAttributedString.Key: Any] {
        var changed = attributes
        changed[key] = value
        return changed
    }
}

/// Text the layer draws itself, cut after `lines` lines with an ellipsis,
/// inside `insets` and over its background color.
final class TextLayer: CALayer {
    private(set) var text = NSAttributedString()
    private var textStyle: TextStyle?
    var lines = 1
    var insets = NSEdgeInsetsZero
    /// The leading a line keeps above and below its glyphs, which the first
    /// and the last line leave out.
    private var leading: CGFloat = 0
    private var lineHeight: CGFloat = 0
    private var measured: (width: CGFloat, size: CGSize)?
    /// A one-line text's own size, which no width changes.
    private var oneLine: CGSize?

    override init() {
        super.init()
        actions = still
        contentsScale = backingScale
        contentsGravity = .topLeft
        needsDisplayOnBoundsChange = false
    }

    override init(layer: Any) {
        super.init(layer: layer)
    }

    required init?(coder: NSCoder) { nil }

    func set(_ string: String, _ style: TextStyle) {
        guard string != text.string || style !== textStyle else { return }
        set(NSAttributedString(string: string, attributes: style.attributes), style)
    }

    func set(_ attributed: NSAttributedString, _ style: TextStyle) {
        text = attributed
        textStyle = style
        lineHeight = style.lineHeight
        leading = (style.lineHeight - style.natural) / 2
        measured = nil
        oneLine = nil
        setNeedsDisplay()
    }

    /// Its size at most `width` wide: as wide as its longest line and as tall
    /// as its lines, at most `lines` of them, with the insets.
    func size(within width: CGFloat) -> CGSize {
        if lines == 1 {
            let own = ownLine()
            return CGSize(width: min(own.width, width), height: own.height)
        }
        if let measured, measured.width == width { return measured.size }
        let size = measure(within: width)
        measured = (width, size)
        return size
    }

    private func ownLine() -> CGSize {
        if let oneLine { return oneLine }
        let own = measure(within: .greatestFiniteMagnitude)
        oneLine = own
        return own
    }

    private func measure(within width: CGFloat) -> CGSize {
        let inner = width - insets.left - insets.right
        let bounds = text.boundingRect(with: CGSize(width: inner, height: .greatestFiniteMagnitude),
                                       options: [.usesLineFragmentOrigin])
        let shown = min((bounds.height / lineHeight).rounded(), CGFloat(lines))
        let height = shown > 0 ? shown * lineHeight - 2 * leading : 0
        return CGSize(width: ceil(bounds.width) + insets.left + insets.right,
                      height: height + insets.top + insets.bottom)
    }

    /// Moves the layer to `rect`; draws the text again only when it looks
    /// different there: a new height, a paragraph's new width, or a line cut
    /// at either width.
    func place(_ rect: CGRect) {
        let old = frame.size
        if rect.size != old {
            let redraw = old.height != rect.height || lines > 1 || ownLine().width > min(old.width, rect.width)
            if redraw { setNeedsDisplay() }
        }
        frame = rect
    }

    override func draw(in context: CGContext) {
        context.saveGState()
        if !contentsAreFlipped() {
            context.translateBy(x: 0, y: bounds.height)
            context.scaleBy(x: 1, y: -1)
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        let inner = NSRect(x: insets.left, y: insets.top - leading, width: bounds.width - insets.left - insets.right,
                           height: bounds.height - insets.top - insets.bottom + 2 * leading)
        text.draw(with: inner, options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine], context: nil)
        NSGraphicsContext.restoreGraphicsState()
        context.restoreGState()
    }
}

/// A view laid out by its parent; its own layers follow from its frame.
class Box: NSView {
    override var isFlipped: Bool { true }

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        layerContentsRedrawPolicy = .never
    }

    required init?(coder: NSCoder) { nil }

    func fill(_ color: NSColor, radius: CGFloat = 0) {
        layer?.backgroundColor = color.cgColor
        layer?.cornerRadius = radius
    }

    func add(_ sublayers: [CALayer]) {
        for sublayer in sublayers { layer?.addSublayer(sublayer) }
    }
}

/// A rounded track filled to a share in a color: the fill is a layer whose
/// width follows the share.
final class Bar {
    let track: CALayer = stillLayer()
    let filled: CALayer = stillLayer()

    init() {
        track.backgroundColor = NSColor.hairline.cgColor
        track.addSublayer(filled)
    }

    func paint(_ color: NSColor) {
        filled.backgroundColor = color.cgColor
    }

    func place(_ rect: CGRect, share: CGFloat) {
        track.frame = rect
        track.cornerRadius = rect.height / 2
        filled.cornerRadius = rect.height / 2
        filled.frame = CGRect(x: 0, y: 0, width: rect.width * min(max(share, 0), 1), height: rect.height)
    }
}

/// Children left to right, wrapping, `spacing` apart both ways, each at its
/// own size: Compose's FlowRow. Returns the height.
func flow(_ sizes: [CGSize], width: CGFloat, spacing: CGFloat, place: (Int, CGPoint) -> Void) -> CGFloat {
    var (x, y, line) = (CGFloat(0), CGFloat(0), CGFloat(0))
    for (index, size) in sizes.enumerated() {
        if x > 0 && x + size.width > width {
            y += line + spacing
            x = 0
            line = 0
        }
        place(index, CGPoint(x: x, y: y))
        x += size.width + spacing
        line = max(line, size.height)
    }
    return y + line
}

/// Every text style the screen uses, at one tier's scale.
struct Styles {
    let tickerSymbol, tickerPrice, tickerUp, tickerDown: TextStyle
    let title, subtitle, body, percent, stat, statLabel, badge, level: TextStyle
    let chips: [TextStyle]
    let bold11: NSFont

    init(_ s: CGFloat) {
        tickerSymbol = TextStyle(10 * s, .ink, bold: true)
        tickerPrice = TextStyle(10 * s, .body)
        tickerUp = TextStyle(10 * s, .up)
        tickerDown = TextStyle(10 * s, .down)
        title = TextStyle(13 * s, .ink, bold: true, lines: 2)
        subtitle = TextStyle(11 * s, .muted)
        body = TextStyle(12 * s, .body, lines: 4)
        percent = TextStyle(10 * s, .muted)
        stat = TextStyle(12 * s, .ink, bold: true)
        statLabel = TextStyle(9 * s, .muted)
        badge = TextStyle(9 * s, .white, bold: true)
        level = TextStyle(9 * s, .white)
        chips = NSColor.gradientEnd.map { TextStyle(10 * s, $0) }
        bold11 = NSFont(name: "Roboto-Bold", size: 11 * s) ?? .boldSystemFont(ofSize: 11 * s)
    }
}

/// One quote: symbol, price, change and a bar, all from the frame.
final class TickerTile {
    let ticker: PerfData.Ticker
    let tile: CALayer = stillLayer()
    let symbol: TextLayer = stillLayer(TextLayer())
    let price: TextLayer = stillLayer(TextLayer())
    let change: TextLayer = stillLayer(TextLayer())
    let bar = Bar()
    let s: CGFloat

    init(ticker: PerfData.Ticker, index: Int, s: CGFloat, styles: Styles) {
        self.ticker = ticker
        self.s = s
        tile.backgroundColor = NSColor.white.cgColor
        tile.cornerRadius = 6 * s
        symbol.set(ticker.symbol, styles.tickerSymbol)
        bar.paint(NSColor.palette[index % 8])
        for sublayer in [symbol, price, change, bar.track] { tile.addSublayer(sublayer) }
    }

    /// Sets the frame's values and returns the tile's size.
    func update(_ frame: Int, styles: Styles) -> CGSize {
        let cents = PerfData.tickerCents(ticker, frame)
        price.set(PerfData.centsText(cents), styles.tickerPrice)
        change.set(PerfData.changeText(ticker, cents), cents >= ticker.baseCents ? styles.tickerUp : styles.tickerDown)
        let texts = [symbol, price, change].map { $0.size(within: .greatestFiniteMagnitude) }
        let height = max(texts.map(\.height).max() ?? 0, 4 * s)
        var x = 6 * s
        for (text, size) in zip([symbol, price, change], texts) {
            text.place(CGRect(x: x, y: 3 * s + (height - size.height) / 2, width: size.width, height: size.height))
            x += size.width + 4 * s
        }
        let share = CGFloat(cents - ticker.baseCents + ticker.swingCents) / CGFloat(2 * ticker.swingCents)
        bar.place(CGRect(x: x, y: 3 * s + (height - 4 * s) / 2, width: 20 * s, height: 4 * s), share: share)
        return CGSize(width: x + 20 * s + 6 * s, height: height + 6 * s)
    }
}

/// The ticker strip: tiles in a flow row whose values change every frame.
final class TickerPanel: Box {
    let tiles: [TickerTile]
    let s: CGFloat

    init(tickers: [PerfData.Ticker], s: CGFloat, styles: Styles) {
        self.s = s
        tiles = tickers.enumerated().map { TickerTile(ticker: $1, index: $0, s: s, styles: styles) }
        super.init(frame: .zero)
        fill(.panel)
        add(tiles.map(\.tile))
    }

    required init?(coder: NSCoder) { nil }

    func place(width: CGFloat, frame: Int, styles: Styles) -> CGFloat {
        let sizes = tiles.map { $0.update(frame, styles: styles) }
        let height = flow(sizes, width: width - 12 * s, spacing: 4 * s) { index, point in
            tiles[index].tile.frame = CGRect(origin: CGPoint(x: point.x + 6 * s, y: point.y + 6 * s), size: sizes[index])
        }
        return height + 12 * s
    }
}

/// A card's 48-point line over a fading fill: two shape layers whose paths
/// follow the frame, the fill a gradient masked by the area under the line.
final class Sparkline {
    let line: CAShapeLayer = stillLayer(CAShapeLayer())
    let fade: CAGradientLayer = stillLayer(CAGradientLayer())
    let area: CAShapeLayer = stillLayer(CAShapeLayer())

    init(s: CGFloat) {
        line.fillColor = nil
        line.lineWidth = 1.5 * s
        fade.mask = area
        fade.startPoint = CGPoint(x: 0.5, y: 0)
        fade.endPoint = CGPoint(x: 0.5, y: 1)
    }

    func paint(_ color: NSColor) {
        line.strokeColor = color.cgColor
        fade.colors = [color.withAlphaComponent(0.25).cgColor, color.withAlphaComponent(0).cgColor]
    }

    func place(_ rect: CGRect, card: Int, frame: Int) {
        let step = rect.width / CGFloat(PerfData.sparkPoints - 1)
        let areaPath = CGMutablePath()
        let linePath = CGMutablePath()
        areaPath.move(to: CGPoint(x: 0, y: rect.height))
        for index in 0..<PerfData.sparkPoints {
            let point = CGPoint(x: CGFloat(index) * step,
                                y: rect.height * CGFloat(1 - PerfData.sparkValue(card, index, frame)))
            areaPath.addLine(to: point)
            if index == 0 { linePath.move(to: point) } else { linePath.addLine(to: point) }
        }
        areaPath.addLine(to: CGPoint(x: rect.width, y: rect.height))
        areaPath.closeSubpath()
        fade.frame = rect
        area.frame = CGRect(origin: .zero, size: rect.size)
        area.path = areaPath
        line.frame = rect
        line.path = linePath
    }
}

/// One post: header, body, progress, sparkline, chips and footer, with a
/// shadow and a border, and every fifth card a tilting badge.
final class CardView: Box {
    static let footerLabels = ["likes", "replies", "shares"]

    let s: CGFloat
    let avatar: CALayer = stillLayer()
    let title: TextLayer = stillLayer(TextLayer())
    let subtitle: TextLayer = stillLayer(TextLayer())
    let body: TextLayer = stillLayer(TextLayer())
    let bar = Bar()
    let percent: TextLayer = stillLayer(TextLayer())
    let sparkline: Sparkline
    let chips: [TextLayer] = (0..<3).map { _ in stillLayer(TextLayer()) }
    let stats: [TextLayer] = (0..<3).map { _ in stillLayer(TextLayer()) }
    let statLabels: [TextLayer] = (0..<3).map { _ in stillLayer(TextLayer()) }
    let dividers: [CALayer] = (0..<2).map { _ in stillLayer() }
    let badge: TextLayer = stillLayer(TextLayer())
    var card = 0

    init(s: CGFloat, styles: Styles) {
        self.s = s
        sparkline = Sparkline(s: s)
        super.init(frame: .zero)
        fill(.white, radius: 12 * s)
        layer?.borderWidth = 1
        layer?.borderColor = NSColor.hairline.cgColor
        layer?.shadowColor = NSColor.black.cgColor
        layer?.shadowOpacity = 0.24
        layer?.shadowRadius = 1.5 * s
        layer?.shadowOffset = CGSize(width: 0, height: -s)
        avatar.cornerRadius = 16 * s
        avatar.masksToBounds = true
        avatar.contentsGravity = .resizeAspectFill
        title.lines = 2
        body.lines = 4
        for chip in chips {
            chip.insets = NSEdgeInsets(top: 3 * s, left: 8 * s, bottom: 3 * s, right: 8 * s)
            chip.cornerRadius = 10 * s
        }
        for (label, text) in zip(statLabels, Self.footerLabels) { label.set(text, styles.statLabel) }
        for divider in dividers { divider.backgroundColor = NSColor.hairline.cgColor }
        badge.insets = NSEdgeInsets(top: 2 * s, left: 6 * s, bottom: 2 * s, right: 6 * s)
        badge.backgroundColor = NSColor.palette[0].cgColor
        badge.cornerRadius = 8 * s
        badge.opacity = 0.9
        badge.set("HOT", styles.badge)
        add([avatar, title, subtitle, body, bar.track, percent, sparkline.fade, sparkline.line] + chips + stats
            + statLabels + dividers + [badge])
    }

    required init?(coder: NSCoder) { nil }

    /// Shows card `card`: its post and avatar, what does not change per frame.
    func bind(card: Int, post: PerfData.Post, avatar image: CGImage, styles: Styles) {
        self.card = card
        avatar.contents = image
        title.set(post.title, styles.title)
        let (tag, tagColor) = post.tags[0]
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        let line = NSMutableAttributedString(string: "by ", attributes: styles.subtitle.attributes)
        line.append(NSAttributedString(string: post.author, attributes: styles.subtitle.with(.font, styles.bold11)))
        line.append(NSAttributedString(string: " · ", attributes: styles.subtitle.attributes))
        line.append(NSAttributedString(string: tag, attributes: styles.subtitle.with(
            .foregroundColor, NSColor.gradientEnd[tagColor])))
        subtitle.set(line, styles.subtitle)
        body.set(post.body, styles.body)
        for (chip, (text, color)) in zip(chips, post.tags) {
            chip.set(text, styles.chips[color])
            chip.backgroundColor = NSColor.chipBackground[color].cgColor
        }
        for (stat, text) in zip(stats, post.stats) { stat.set(text, styles.stat) }
        sparkline.paint(NSColor.palette[post.color])
        bar.paint(NSColor.palette[card % 8])
        badge.isHidden = card % 5 != 0
    }

    /// Lays the card out `width` wide on `frame`; returns its height.
    func place(width: CGFloat, frame: Int, styles: Styles) -> CGFloat {
        let inner = width - 20 * s
        var y = 10 * s
        // Header: the avatar beside the title and subtitle, centered.
        let textWidth = inner - 40 * s
        let titleSize = title.size(within: textWidth)
        let subtitleHeight = subtitle.size(within: textWidth).height
        let header = max(32 * s, titleSize.height + subtitleHeight)
        avatar.frame = CGRect(x: 10 * s, y: y + (header - 32 * s) / 2, width: 32 * s, height: 32 * s)
        let top = y + (header - titleSize.height - subtitleHeight) / 2
        title.place(CGRect(x: 50 * s, y: top, width: textWidth, height: titleSize.height))
        subtitle.place(CGRect(x: 50 * s, y: top + titleSize.height, width: textWidth, height: subtitleHeight))
        y += header + 6 * s
        let bodyHeight = body.size(within: inner).height
        body.place(CGRect(x: 10 * s, y: y, width: inner, height: bodyHeight))
        y += bodyHeight + 6 * s
        // Progress: the bar takes what the percent leaves.
        let permille = PerfData.progressPermille(card, frame)
        percent.set("\(permille / 10)%", styles.percent)
        let percentSize = percent.size(within: .greatestFiniteMagnitude)
        let row = max(percentSize.height, 6 * s)
        let barWidth = inner - percentSize.width - 6 * s
        bar.place(CGRect(x: 10 * s, y: y + (row - 6 * s) / 2, width: barWidth, height: 6 * s),
                  share: CGFloat(permille) / 1000)
        percent.place(CGRect(x: 10 * s + barWidth + 6 * s, y: y + (row - percentSize.height) / 2,
                             width: percentSize.width, height: percentSize.height))
        y += row + 6 * s
        sparkline.place(CGRect(x: 10 * s, y: y, width: inner, height: 36 * s), card: card, frame: frame)
        y += 36 * s + 6 * s
        let chipSizes = chips.map { $0.size(within: inner) }
        let chipsTop = y
        let chipsHeight = flow(chipSizes, width: inner, spacing: 4 * s) { index, point in
            chips[index].place(CGRect(origin: CGPoint(x: point.x + 10 * s, y: point.y + chipsTop), size: chipSizes[index]))
        }
        y += chipsHeight + 6 * s
        // Footer: three counters split by dividers as tall as the tallest.
        let column = (inner - 2) / 3
        let statSizes = stats.map { $0.size(within: column) }
        let labelSizes = statLabels.map { $0.size(within: column) }
        let footer = zip(statSizes, labelSizes).map { $0.height + $1.height }.max() ?? 0
        for index in 0..<3 {
            let x = 10 * s + CGFloat(index) * (column + 1)
            stats[index].place(CGRect(x: x + (column - statSizes[index].width) / 2, y: y,
                                      width: statSizes[index].width, height: statSizes[index].height))
            statLabels[index].place(CGRect(x: x + (column - labelSizes[index].width) / 2,
                                           y: y + statSizes[index].height, width: labelSizes[index].width,
                                           height: labelSizes[index].height))
            if index > 0 { dividers[index - 1].frame = CGRect(x: x - 1, y: y, width: 1, height: footer) }
        }
        y += footer + 10 * s
        if card % 5 == 0 {
            // A translucent tag tilting with the frame: rotated, never laid out again.
            let size = badge.size(within: .greatestFiniteMagnitude)
            badge.setAffineTransform(.identity)
            badge.place(CGRect(x: width - 6 * s - size.width, y: 6 * s, width: size.width, height: size.height))
            badge.setAffineTransform(CGAffineTransform(rotationAngle: PerfData.badgeDegrees(card, frame) * .pi / 180))
        }
        layer?.shadowPath = CGPath(roundedRect: CGRect(x: 0, y: 0, width: width, height: y),
                                   cornerWidth: 12 * s, cornerHeight: 12 * s, transform: nil)
        return y
    }
}

/// One level of a cluster: its background, three labels above the next
/// level.
final class Level {
    let remaining: Int
    let box: CALayer = stillLayer()
    let labels: [TextLayer] = (0..<3).map { _ in stillLayer(TextLayer()) }
    let inner: Level?
    let s: CGFloat

    init(remaining: Int, s: CGFloat) {
        self.remaining = remaining
        self.s = s
        inner = remaining > 0 ? Level(remaining: remaining - 1, s: s) : nil
        box.backgroundColor = NSColor.levelBackground[remaining % 2].cgColor
        box.cornerRadius = 4 * s
        for label in labels {
            label.cornerRadius = 2 * s
            box.addSublayer(label)
        }
        if let inner { box.addSublayer(inner.box) }
    }

    func bind(cluster: Int, styles: Styles) {
        for (chip, label) in labels.enumerated() {
            label.set("C\(cluster).L\(remaining).\(chip)", styles.level)
            label.backgroundColor = NSColor.palette[(remaining + chip + cluster) % 8].cgColor
        }
        inner?.bind(cluster: cluster, styles: styles)
    }

    /// Lays the level out `width` wide; returns its height.
    func place(width: CGFloat) -> CGFloat {
        let content = width - 4 * s
        let column = (content - 4 * s) / 3
        let height = labels.map { $0.size(within: column).height }.max() ?? 0
        for (index, label) in labels.enumerated() {
            label.place(CGRect(x: 3 * s + CGFloat(index) * (column + 2 * s), y: s, width: column, height: height))
        }
        var y = s + height
        if let inner {
            y += s
            let innerHeight = inner.place(width: content)
            inner.box.frame = CGRect(x: 3 * s, y: y, width: content, height: innerHeight)
            y += innerHeight
        }
        return y + s
    }
}

/// A list row: a row of cards, or a deep cluster.
final class RowView: Box {
    let cards: [CardView]
    let level: Level?

    init(cards: [CardView]) {
        self.cards = cards
        level = nil
        super.init(frame: .zero)
        for card in cards { addSubview(card) }
    }

    init(level: Level) {
        cards = []
        self.level = level
        super.init(frame: .zero)
        add([level.box])
    }

    required init?(coder: NSCoder) { nil }

    func place(width: CGFloat, frame: Int, s: CGFloat, styles: Styles) -> CGFloat {
        if let level {
            let height = level.place(width: width)
            level.box.frame = CGRect(x: 0, y: 0, width: width, height: height)
            return height
        }
        let column = (width - 8 * s * CGFloat(cards.count - 1)) / CGFloat(cards.count)
        var tallest: CGFloat = 0
        for (index, card) in cards.enumerated() {
            let height = card.place(width: column, frame: frame, styles: styles)
            card.frame = CGRect(x: CGFloat(index) * (column + 8 * s), y: 0, width: column, height: height)
            tallest = max(tallest, height)
        }
        return tallest
    }
}

/// The data the screen draws, made once.
final class Model {
    let tier = PerfData.tier(Launch.tier)
    let posts = PerfData.posts()
    let tickers: [PerfData.Ticker]
    let avatars: [CGImage]
    let styles: Styles

    init() {
        tickers = PerfData.tickers(tier.tickers)
        styles = Styles(tier.scale)
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        avatars = (0..<PerfData.avatarCount).compactMap { index in
            let size = PerfData.avatarSize
            guard let provider = CGDataProvider(data: Data(PerfData.avatarRGBA(index)) as CFData) else { return nil }
            return CGImage(width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: size * 4,
                           space: space, bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                           provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        }
    }
}

/// The list: rows from the first one on screen down to the window's bottom,
/// each recycled once it scrolled off, the offset 3 points a frame.
final class ListView: Box {
    let model: Model
    private var anchor = 0
    private var anchorTop: CGFloat
    private var shown: [Int: RowView] = [:]
    private var spareCards: [RowView] = []
    private var spareClusters: [RowView] = []

    init(model: Model) {
        self.model = model
        anchorTop = 8 * model.tier.scale
        super.init(frame: .zero)
        layer?.masksToBounds = true
    }

    required init?(coder: NSCoder) { nil }

    private func view(for row: Int) -> RowView {
        if let view = shown[row] { return view }
        let s = model.tier.scale
        let view: RowView
        switch PerfData.row(row, columns: model.tier.columns) {
        case .cards(let first):
            view = spareCards.popLast() ?? RowView(cards: (0..<model.tier.columns).map { _ in
                CardView(s: s, styles: model.styles)
            })
            for (index, card) in view.cards.enumerated() {
                let number = first + index
                card.bind(card: number, post: model.posts[number % PerfData.postCount],
                          avatar: model.avatars[number % PerfData.avatarCount], styles: model.styles)
            }
        case .cluster(let cluster):
            view = spareClusters.popLast() ?? RowView(level: Level(remaining: model.tier.depth, s: s))
            view.level?.bind(cluster: cluster, styles: model.styles)
        }
        addSubview(view)
        shown[row] = view
        return view
    }

    private func recycle(_ row: Int) {
        guard let view = shown.removeValue(forKey: row) else { return }
        view.removeFromSuperview()
        if view.level == nil { spareCards.append(view) } else { spareClusters.append(view) }
    }

    func place(size: CGSize, frame: Int) {
        let s = model.tier.scale
        let gap = 8 * s
        let offset = CGFloat(frame) * 3
        var (row, top) = (anchor, anchorTop - offset)
        while top < size.height {
            let view = view(for: row)
            let height = view.place(width: size.width - 2 * gap, frame: frame, s: s, styles: model.styles)
            view.frame = CGRect(x: gap, y: top, width: size.width - 2 * gap, height: height)
            if top + height + gap <= 0 && row == anchor {
                // Scrolled off above: built no more.
                recycle(row)
                anchor = row + 1
                anchorTop += height + gap
            }
            top += height + gap
            row += 1
        }
        for stale in shown.keys where stale >= row { recycle(stale) }
    }
}

/// The window's root: the top bar, then the ticker strip and the list in a
/// column whose width follows the frame.
final class GauntletView: Box {
    let model: Model
    let topBar = Box()
    let heading: TextLayer = stillLayer(TextLayer())
    let panel: TickerPanel
    let list: ListView
    private var link: CADisplayLink?

    init() {
        let model = Model()
        self.model = model
        panel = TickerPanel(tickers: model.tickers, s: model.tier.scale, styles: model.styles)
        list = ListView(model: model)
        super.init(frame: CGRect(origin: .zero, size: Launch.window))
        fill(NSColor(rgb: 0xEEF0F5))
        topBar.fill(NSColor(rgb: 0x1E2A4A))
        heading.set("Gauntlet", TextStyle(20, .white, bold: true))
        topBar.add([heading])
        for view in [topBar, panel, list] { addSubview(view) }
    }

    required init?(coder: NSCoder) { nil }

    func start() {
        let link = displayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        if Clock.frame == 0 { log("PERF first_frame") }
        Clock.frame += 1
        needsLayout = true
        if Launch.freeze > 0 && Clock.frame >= Launch.freeze {
            log("PERF frozen frame=\(Clock.frame)")
            link.invalidate()
        }
    }

    override func layout() {
        super.layout()
        let frame = Clock.frame
        topBar.frame = CGRect(x: 0, y: 0, width: bounds.width, height: 56)
        let headingSize = heading.size(within: .greatestFiniteMagnitude)
        heading.place(CGRect(x: 16, y: (56 - headingSize.height) / 2, width: headingSize.width,
                             height: headingSize.height))
        let width = (bounds.width * PerfData.widthFraction(frame)).rounded()
        let panelHeight = panel.place(width: width, frame: frame, styles: model.styles)
        panel.frame = CGRect(x: 0, y: 56, width: width, height: panelHeight)
        let listSize = CGSize(width: width, height: bounds.height - 56 - panelHeight)
        list.frame = CGRect(origin: CGPoint(x: 0, y: 56 + panelHeight), size: listSize)
        list.place(size: listSize, frame: frame)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        backingScale = NSScreen.main?.backingScaleFactor ?? 2
        let root = GauntletView()
        let window = NSWindow(contentRect: CGRect(origin: .zero, size: Launch.window),
                              styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Gauntlet"
        window.contentView = root
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        self.window = window
        root.start()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
enum GauntletApp {
    static func main() {
        // Roboto is no macOS system font: the app registers the files every
        // desktop app loads, for this process only.
        for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
            let url = URL(fileURLWithPath: Launch.fonts).appendingPathComponent(file)
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}

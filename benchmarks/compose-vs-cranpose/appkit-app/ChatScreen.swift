// The chat beside the patchwork in AppKit: an agent's answers arrive from a
// data thread as tokens and the list shows them as they come.
//
// In AppKit's idiom each block is a view made once, when its first token
// arrives. A paragraph, a heading or the code is a TextKit text view whose
// storage a token appends to, so the layout manager lays out the new
// characters and the view draws what they changed. A table adds a row layer
// a token. The blocks stay where they were put; one container moves to keep
// the end of the list in view, and the blocks of turns that scrolled out of
// sight are dropped. The animated picture's frames are decoded once and
// set as a layer's contents. The status line's spinner is a layer that turns
// with a transform, and its text is a gradient layer, which moves, under a
// text layer used as its mask.

import AppKit
import QuartzCore

/// The chat's text styles, made once.
enum ChatStyles {
    static let text = TextStyle(ChatData.textSize, .white, lines: 2)
    static let bold = TextStyle(ChatData.textSize, .white, bold: true, lines: 2)
    static let heading = TextStyle(ChatData.headingSize, .white, bold: true, lines: 2)
    static let code = TextStyle(ChatData.codeSize, .white, mono: true, lines: 2)
    static let cell = TextStyle(ChatData.codeSize, .white)
    static let headerCell = TextStyle(ChatData.codeSize, .white, bold: true)
    static let panel = NSColor(rgb: ChatData.panelRGB).cgColor
    static let bubble = NSColor(rgb: ChatData.userRGB).cgColor
}

/// A block of the chat, a view of the list's width; the chat puts it at `top`.
class BlockItem: Box {
    var top: CGFloat = 0
    fileprivate(set) var height: CGFloat = 0

    /// Shows `shown` tokens of the block.
    func show(_ shown: Int) {}
}

/// A bubble of text on a fill, as wide as its text.
func bubble(_ text: String, _ style: TextStyle, radius: CGFloat) -> TextLayer {
    let layer: TextLayer = stillLayer(TextLayer())
    layer.lines = 8
    layer.insets = NSEdgeInsets(top: 6, left: 10, bottom: 6, right: 10)
    layer.set(text, style)
    layer.backgroundColor = ChatStyles.bubble
    layer.cornerRadius = radius
    return layer
}

final class UserBlock: BlockItem {
    init(_ text: String) {
        super.init(frame: .zero)
        let layer = bubble(text, ChatStyles.text, radius: 10)
        let size = layer.size(within: ChatView.inner)
        layer.place(CGRect(x: ChatView.inner - size.width, y: 0, width: size.width, height: size.height))
        self.layer?.addSublayer(layer)
        height = size.height
    }

    required init?(coder: NSCoder) { nil }
}

/// A paragraph, a heading or code: the mark of the turn, then as much of the
/// text as arrived, in a text view that each token appends to.
final class TypedBlock: BlockItem {
    private let storage: NSTextStorage
    private let layout: NSLayoutManager
    private let container: NSTextContainer
    private let textView: NSTextView
    private let content: NSAttributedString
    private let text: String
    private let head: Int
    private let leading: CGFloat
    private let inset: CGFloat
    private var shown = 0

    /// `content` is the mark and then the text; `head` characters of it are the mark.
    init(_ content: NSAttributedString, head: Int, text: String, style: TextStyle, panel: Bool) {
        self.content = content
        self.text = text
        self.head = head
        leading = (style.lineHeight - style.natural) / 2
        inset = panel ? 8 : 0
        let (storage, layout, container) = (NSTextStorage(), NSLayoutManager(), NSTextContainer())
        storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        container.lineFragmentPadding = 0
        container.widthTracksTextView = false
        container.containerSize = CGSize(width: ChatView.inner - 2 * inset, height: 1e7)
        (self.storage, self.layout, self.container) = (storage, layout, container)
        textView = NSTextView(frame: .zero, textContainer: container)
        super.init(frame: .zero)
        textView.isEditable = false
        textView.isSelectable = false
        textView.drawsBackground = false
        textView.textContainerInset = .zero
        textView.isVerticallyResizable = false
        textView.isHorizontallyResizable = false
        textView.layerContentsRedrawPolicy = .onSetNeedsDisplay
        if panel { fill(NSColor(rgb: ChatData.panelRGB), radius: 6) }
        addSubview(textView)
    }

    required init?(coder: NSCoder) { nil }

    override func show(_ tokens: Int) {
        let count = head + ChatData.shownChars(text, tokens)
        guard count > shown else { return }
        storage.append(content.attributedSubstring(from: NSRange(location: shown, length: count - shown)))
        shown = count
        layout.ensureLayout(for: container)
        let used = layout.usedRect(for: container).height
        if textView.frame.height != used {
            textView.frame = CGRect(x: inset, y: inset - leading, width: ChatView.inner - 2 * inset, height: used)
        }
        height = used - 2 * leading + 2 * inset
    }
}

/// A table whose rows appear a token each.
final class TableBlock: BlockItem {
    private let rows: [[String]]
    private let rowHeight = ChatStyles.cell.natural + 6
    private var shownRows = 0

    init(_ rows: [[String]]) {
        self.rows = rows
        super.init(frame: .zero)
    }

    required init?(coder: NSCoder) { nil }

    override func show(_ tokens: Int) {
        let count = min(tokens, rows.count)
        for index in shownRows..<max(count, shownRows) {
            let row = stillLayer()
            row.frame = CGRect(x: 0, y: CGFloat(index) * rowHeight, width: ChatView.inner, height: rowHeight)
            if index == 0 || index % 2 == 1 { row.backgroundColor = ChatStyles.panel }
            let width = (ChatView.inner - 12) / CGFloat(rows[index].count)
            for (column, cell) in rows[index].enumerated() {
                let text: TextLayer = stillLayer(TextLayer())
                text.set(cell, index == 0 ? ChatStyles.headerCell : ChatStyles.cell)
                text.frame = CGRect(x: 6 + CGFloat(column) * width, y: 3, width: width, height: ChatStyles.cell.natural)
                row.addSublayer(text)
            }
            layer?.addSublayer(row)
        }
        shownRows = max(count, shownRows)
        height = CGFloat(shownRows) * rowHeight
    }
}

/// The animated picture, which the chat sets to a frame.
final class GifBlock: BlockItem {
    private let picture: CALayer = stillLayer()

    init() {
        super.init(frame: .zero)
        picture.frame = CGRect(x: 0, y: 0, width: ChatData.gifSize, height: ChatData.gifSize)
        picture.contentsGravity = .resize
        layer?.addSublayer(picture)
        height = ChatData.gifSize
    }

    required init?(coder: NSCoder) { nil }

    func set(_ image: CGImage) {
        picture.contents = image
    }
}

/// A row of buttons.
final class ButtonsBlock: BlockItem {
    init(_ labels: [String]) {
        super.init(frame: .zero)
        var x: CGFloat = 0
        for label in labels {
            let button = bubble(label, ChatStyles.bold, radius: 6)
            let size = button.size(within: ChatView.inner)
            button.place(CGRect(x: x, y: 0, width: size.width, height: size.height))
            layer?.addSublayer(button)
            x += size.width + 8
            height = max(height, size.height)
        }
    }

    required init?(coder: NSCoder) { nil }
}

/// The chat's column: the list of blocks over the status line.
final class ChatView: Box {
    /// The width of a block, inside the list's padding.
    static let inner = ChatData.width - 16
    private static let padding: CGFloat = 8

    private let viewport = Box()
    private let list = Box()
    private let status = StatusLine()
    private let feed = ChatFeed()
    private let gifFrames = ChatAssets.gifFrames(in: Launch.chat)
    private let viewHeight: CGFloat
    /// The blocks of the last two turns, and the chat's number of the first.
    private var blocks: [BlockItem] = []
    private var first = 0
    private var bottom = ChatView.padding
    private var live = (block: -1, shown: 0)
    private var picture = -1
    private var token: ChatData.Token?

    init(height: CGFloat) {
        viewHeight = height - ChatData.statusHeight
        super.init(frame: CGRect(x: 0, y: 0, width: ChatData.width, height: height))
        fill(NSColor(rgb: ChatData.backgroundRGB))
        viewport.frame = CGRect(x: 0, y: 0, width: ChatData.width, height: viewHeight)
        viewport.layer?.masksToBounds = true
        viewport.addSubview(list)
        addSubview(viewport)
        status.frame = CGRect(x: 0, y: viewHeight, width: ChatData.width, height: ChatData.statusHeight)
        addSubview(status)
    }

    required init?(coder: NSCoder) { nil }

    /// Takes the frame's tokens. The screen never waits for a token.
    func advance(_ frame: Int) {
        for _ in 0..<ChatData.tokensPerFrame {
            guard let arrived = feed.take() else { break }
            if arrived.block != live.block {
                flush()
                begin(arrived.block)
            }
            live.shown = arrived.shown
            token = arrived
        }
        flush()
        if let token { status.show(token) }
        status.advance(frame)
        let next = ChatData.gifFrame(frame, gifFrames.count)
        if next != picture, !gifFrames.isEmpty {
            picture = next
            for case let block as GifBlock in blocks { block.set(gifFrames[next]) }
        }
    }

    /// Shows the live block's tokens and keeps the end of the list in view.
    private func flush() {
        guard let block = blocks.last, live.block >= 0 else { return }
        block.show(live.shown)
        bottom = block.top + block.height
        if block.frame.height != block.height || block.frame.origin.y != block.top {
            block.frame = CGRect(x: ChatView.padding, y: block.top, width: ChatView.inner, height: block.height)
        }
        list.frame = CGRect(x: 0, y: -max(0, bottom + ChatView.padding - viewHeight), width: ChatData.width, height: 0)
    }

    private func begin(_ index: Int) {
        live = (index, 0)
        let turn = ChatData.turnOf(index)
        let block = makeBlock(ChatData.turn[index % ChatData.turn.count], turn: turn)
        block.top = blocks.isEmpty ? ChatView.padding : bottom + ChatView.padding
        if let gif = block as? GifBlock, picture >= 0 { gif.set(gifFrames[picture]) }
        blocks.append(block)
        list.addSubview(block)
        // Blocks of turns before the last two lie above the view.
        let oldest = max(turn - 1, 0) * ChatData.turn.count
        if first < oldest {
            let gone = oldest - first
            let shift = blocks[gone].top - ChatView.padding
            for block in blocks.prefix(gone) { block.removeFromSuperview() }
            blocks.removeFirst(gone)
            first = oldest
            for block in blocks {
                block.top -= shift
                block.frame.origin.y = block.top
            }
            bottom -= shift
        }
    }

    private func makeBlock(_ block: ChatData.Block, turn: Int) -> BlockItem {
        let mark = ChatData.turnLabel(turn)
        switch block {
        case .user(let text): return UserBlock(text)
        case .heading(let text):
            return typed(mark + " ", text, ChatStyles.heading, panel: false) { _ in }
        case .paragraph(let text):
            return typed(mark + " ", text, ChatStyles.text, panel: false) { _ in }
        case .code(let code):
            let head = "// " + mark + "\n"
            return typed(head, code, ChatStyles.code, panel: true) { content in
                content.addAttribute(.foregroundColor, value: NSColor(rgb: ChatData.CodeKind.comment.rgb),
                                     range: NSRange(location: 0, length: head.utf8.count))
                for (range, kind) in ChatData.codeSpans(code) {
                    content.addAttribute(.foregroundColor, value: NSColor(rgb: kind.rgb),
                                         range: NSRange(location: head.utf8.count + range.lowerBound, length: range.count))
                }
            }
        case .table(let rows): return TableBlock(rows)
        case .gif: return GifBlock()
        case .buttons(let labels): return ButtonsBlock(labels)
        }
    }

    /// A typed block of `head`, then `text`, in `style`; `color` sets the text's colors.
    private func typed(_ head: String, _ text: String, _ style: TextStyle, panel: Bool,
                       color: (NSMutableAttributedString) -> Void) -> TypedBlock {
        let content = NSMutableAttributedString(string: head + text, attributes: style.attributes)
        color(content)
        return TypedBlock(content, head: head.utf8.count, text: text, style: style, panel: panel)
    }
}

/// The status line under the list: a spinner that turns each frame and what
/// the agent does, under a gradient that moves each frame.
final class StatusLine: Box {
    private static let textX = 8 + ChatData.spinnerSize + 6
    private let spinner: CALayer = stillLayer()
    private let glow: CALayer = stillLayer()
    private let shade: CAGradientLayer = stillLayer(CAGradientLayer())
    private let text: TextLayer = stillLayer(TextLayer())
    private let style = TextStyle(ChatData.textSize, .white)
    private var counted = 0

    init() {
        super.init(frame: .zero)
        fill(NSColor(rgb: ChatData.panelRGB))
        spinner.frame = CGRect(x: 8, y: (ChatData.statusHeight - ChatData.spinnerSize) / 2,
                               width: ChatData.spinnerSize, height: ChatData.spinnerSize)
        spinner.contents = ChatAssets.spinner(in: Launch.chat, side: ChatData.spinnerSize)
        glow.frame = CGRect(x: StatusLine.textX, y: 0, width: ChatData.width - StatusLine.textX - 8,
                            height: ChatData.statusHeight)
        text.frame = CGRect(x: 0, y: (ChatData.statusHeight - style.natural) / 2, width: glow.bounds.width,
                            height: style.natural)
        // Four passes of dim, bright, dim, wider than the text, shifted by the frame.
        let dim = NSColor(rgb: ChatData.shimmerDimRGB).cgColor
        shade.colors = (0...8).map { $0 % 2 == 0 ? dim : NSColor.white.cgColor }
        shade.startPoint = CGPoint(x: 0, y: 0.5)
        shade.endPoint = CGPoint(x: 1, y: 0.5)
        glow.mask = text
        glow.addSublayer(shade)
        layer?.addSublayer(spinner)
        layer?.addSublayer(glow)
    }

    required init?(coder: NSCoder) { nil }

    func show(_ token: ChatData.Token) {
        guard token.count != counted else { return }
        counted = token.count
        text.set(ChatData.status(token), style)
    }

    func advance(_ frame: Int) {
        let degrees = Double(frame) * ChatData.spinDegrees
        spinner.setAffineTransform(CGAffineTransform(rotationAngle: CGFloat(degrees * .pi / 180)))
        let start = (Double(frame) * ChatData.shimmerStep).truncatingRemainder(dividingBy: ChatData.shimmerSpan)
        shade.frame = CGRect(x: start - ChatData.shimmerSpan, y: 0, width: 4 * ChatData.shimmerSpan,
                             height: ChatData.statusHeight)
    }
}

// The chat beside the patchwork in SwiftUI: an agent's answers arrive from a
// data thread as tokens and the list shows them as they come.
//
// In SwiftUI's idiom the list reads only the number of blocks, so a block
// beginning adds one row to a `LazyVStack` and a token evaluates the last
// block alone, the one view that reads how much of it shows. A `ScrollPosition`
// follows the content's height to keep the list at its end. The code is colored once as an `AttributedString`.
// The animated picture's frames are decoded once and the picture reads its
// frame index. The status line's spinner turns with `.rotationEffect`, and its
// text takes its gradient from a `TextRenderer`.

import Observation
import SwiftUI

/// What the chat shows: its blocks so far, how much of the last one and the
/// last token, which `advance` writes as tokens arrive from the data thread.
@Observable
final class ChatModel {
    private(set) var blocks = 0
    private(set) var shown = 0
    private(set) var token: ChatData.Token?
    private(set) var gif = 0
    @ObservationIgnored private let feed = ChatFeed()
    @ObservationIgnored let gifFrames = ChatAssets.gifFrames(in: Launch.chat)
    @ObservationIgnored let spinner = ChatAssets.spinner(in: Launch.chat, side: ChatData.spinnerSize)
    /// Each code block's text with its colors, made once.
    @ObservationIgnored let highlighted: [AttributedString] = ChatData.turn.map { block in
        guard case .code(let code) = block else { return AttributedString() }
        var text = AttributedString()
        var at = 0
        let bytes = Array(code.utf8)
        func append(_ range: Range<Int>, _ rgb: UInt32?) {
            var piece = AttributedString(String(decoding: bytes[range], as: UTF8.self))
            if let rgb { piece.foregroundColor = Color(rgb: rgb) }
            text.append(piece)
        }
        for (range, kind) in ChatData.codeSpans(code) {
            append(at..<range.lowerBound, nil)
            append(range, kind.rgb)
            at = range.upperBound
        }
        append(at..<bytes.count, nil)
        return text
    }

    /// Takes the frame's tokens. The screen never waits for a token.
    func advance(_ frame: Int) {
        for _ in 0..<ChatData.tokensPerFrame {
            guard let arrived = feed.take() else { break }
            blocks = arrived.block + 1
            shown = arrived.shown
            token = arrived
        }
        let picture = ChatData.gifFrame(frame, gifFrames.count)
        if picture != gif { gif = picture }
    }
}

struct ChatView: View {
    let chat: ChatModel
    let clock: Clock

    var body: some View {
        VStack(spacing: 0) {
            ChatList(chat: chat)
            StatusLine(chat: chat, clock: clock)
        }
        .frame(width: ChatData.width)
        .frame(maxHeight: .infinity)
        .background(Color(rgb: ChatData.backgroundRGB))
    }
}

/// The list of blocks: it reads their number, so it evaluates again when a
/// block begins and not for the tokens of a block.
struct ChatList: View {
    let chat: ChatModel
    @State private var position = ScrollPosition(edge: .bottom)

    var body: some View {
        let count = chat.blocks
        ScrollView(.vertical, showsIndicators: false) {
            LazyVStack(alignment: .leading, spacing: 8) {
                ForEach(0..<count, id: \.self) { index in
                    BlockView(index: index, live: index + 1 == count, chat: chat)
                }
            }
            .padding(8)
        }
        .scrollPosition($position)
        .onScrollGeometryChange(for: CGFloat.self) { $0.contentSize.height } action: { _, _ in
            position.scrollTo(edge: .bottom)
        }
    }
}

extension Color {
    static let bubble = Color(rgb: ChatData.userRGB)
    static let chatPanel = Color(rgb: ChatData.panelRGB)
}

/// Block `index` of the chat; `live` while it is the last one, the one that
/// reads how much of it shows.
struct BlockView: View {
    let index: Int
    let live: Bool
    let chat: ChatModel

    var body: some View {
        let turn = ChatData.turnOf(index)
        switch ChatData.turn[index % ChatData.turn.count] {
        case .user(let text):
            HStack(spacing: 0) {
                Spacer(minLength: 0)
                Text(text).font(roboto(ChatData.textSize, 1)).foregroundStyle(.white)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Color.bubble, in: RoundedRectangle(cornerRadius: 10))
            }
        case .heading(let text): TypedText(turn: turn, text: text, heading: true, live: live, chat: chat)
        case .paragraph(let text): TypedText(turn: turn, text: text, heading: false, live: live, chat: chat)
        case .code(let code):
            CodeText(turn: turn, code: code, colored: chat.highlighted[index % ChatData.turn.count], live: live, chat: chat)
        case .table(let rows): TableView(rows: rows, live: live, chat: chat)
        case .gif: GifView(chat: chat)
        case .buttons(let labels):
            HStack(spacing: 8) {
                ForEach(labels, id: \.self) { label in
                    Button {} label: {
                        Text(label).font(roboto(ChatData.textSize, 1, bold: true)).foregroundStyle(.white)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .background(Color.bubble, in: RoundedRectangle(cornerRadius: 6))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

/// A paragraph, or a heading in the bold face: the turn's number, then as
/// much of the text as arrived.
struct TypedText: View {
    let turn: Int
    let text: String
    let heading: Bool
    let live: Bool
    let chat: ChatModel

    var body: some View {
        let chars = live ? ChatData.shownChars(text, chat.shown) : text.utf8.count
        Text(verbatim: ChatData.turnLabel(turn) + " " + text.prefix(chars))
            .font(roboto(heading ? ChatData.headingSize : ChatData.textSize, 1, bold: heading))
            .lineHeight(heading ? ChatData.headingSize : ChatData.textSize)
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct CodeText: View {
    let turn: Int
    let code: String
    let colored: AttributedString
    let live: Bool
    let chat: ChatModel

    var body: some View {
        let chars = live ? ChatData.shownChars(code, chat.shown) : code.utf8.count
        var text = AttributedString("// " + ChatData.turnLabel(turn) + "\n")
        text.foregroundColor = Color(rgb: ChatData.CodeKind.comment.rgb)
        text.append(AttributedString(colored[..<colored.index(colored.startIndex, offsetByCharacters: chars)]))
        return Text(text).font(.custom("DejaVu Sans Mono", fixedSize: ChatData.codeSize))
            .lineHeight(ChatData.codeSize, natural: 1.164).foregroundStyle(.white)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(8)
            .background(Color.chatPanel, in: RoundedRectangle(cornerRadius: 6))
    }
}

struct TableView: View {
    let rows: [[String]]
    let live: Bool
    let chat: ChatModel

    var body: some View {
        let count = live ? min(chat.shown, rows.count) : rows.count
        VStack(spacing: 0) {
            ForEach(0..<count, id: \.self) { index in
                TableRow(cells: rows[index], header: index == 0, shaded: index % 2 == 1)
            }
        }
    }
}

struct TableRow: View {
    let cells: [String]
    let header: Bool
    let shaded: Bool

    var body: some View {
        HStack(spacing: 0) {
            ForEach(0..<cells.count, id: \.self) { column in
                Text(cells[column]).font(roboto(ChatData.codeSize, 1, bold: header)).foregroundStyle(.white)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .background(header || shaded ? Color.chatPanel : .clear)
    }
}

/// The animated picture: the one view of its block that reads the frame.
struct GifView: View {
    let chat: ChatModel

    var body: some View {
        if !chat.gifFrames.isEmpty {
            Image(decorative: chat.gifFrames[chat.gif], scale: 1).resizable()
                .frame(width: ChatData.gifSize, height: ChatData.gifSize)
        }
    }
}

/// The status line under the list: a spinner that turns each frame and what
/// the agent does, under a gradient that moves each frame.
struct StatusLine: View {
    let chat: ChatModel
    let clock: Clock

    var body: some View {
        HStack(spacing: 6) {
            if let spinner = chat.spinner {
                Spinner(image: spinner, clock: clock)
            }
            StatusText(chat: chat, clock: clock)
        }
        .padding(.horizontal, 8)
        .frame(width: ChatData.width, height: ChatData.statusHeight, alignment: .leading)
        .background(Color.chatPanel)
    }
}

struct Spinner: View {
    let image: CGImage
    let clock: Clock

    var body: some View {
        Image(decorative: image, scale: CGFloat(image.width) / ChatData.spinnerSize).resizable()
            .frame(width: ChatData.spinnerSize, height: ChatData.spinnerSize)
            .rotationEffect(.degrees(Double(clock.frame) * ChatData.spinDegrees))
    }
}

struct StatusText: View {
    let chat: ChatModel
    let clock: Clock

    var body: some View {
        if let token = chat.token {
            Text(verbatim: ChatData.status(token)).font(roboto(ChatData.textSize, 1))
                .textRenderer(Shimmer(start: (Double(clock.frame) * ChatData.shimmerStep)
                        .truncatingRemainder(dividingBy: ChatData.shimmerSpan)))
        }
    }
}

/// Draws the text's glyphs under a gradient that runs dim, bright, dim over
/// `ChatData.shimmerSpan` points, again and again, from `start`.
struct Shimmer: TextRenderer {
    var start: Double

    func draw(layout: Text.Layout, in context: inout GraphicsContext) {
        let dim = Color(rgb: ChatData.shimmerDimRGB)
        context.drawLayer { layer in
            for line in layout { layer.draw(line) }
            layer.blendMode = .sourceIn
            for pass in 0..<4 {
                let left = start + (Double(pass) - 1) * ChatData.shimmerSpan
                layer.fill(
                    Path(CGRect(x: left, y: -50, width: ChatData.shimmerSpan, height: 100)),
                    with: .linearGradient(Gradient(colors: [dim, .white, dim]),
                                          startPoint: CGPoint(x: left, y: 0),
                                          endPoint: CGPoint(x: left + ChatData.shimmerSpan, y: 0)))
            }
        }
    }
}

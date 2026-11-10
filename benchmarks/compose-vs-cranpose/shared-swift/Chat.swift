// The chat beside the patchwork, as `perf-data/src/chat.rs` makes it bit for
// bit: an agent's answers arrive from a data thread as tokens, several a
// frame, and the screen shows them as they come. Under the list a status
// line says what the agent does.

import Foundation

enum ChatData {
    static let width: CGFloat = 320
    static let tokensPerFrame = 4
    static let tokenChars = 6
    static let gifHold = 2
    static let statusHeight: CGFloat = 28
    static let spinnerSize: CGFloat = 16
    static let spinDegrees = 12.0
    static let shimmerStep = 6.0
    static let shimmerSpan = 120.0
    static let gifSize: CGFloat = 64
    static let textSize: CGFloat = 12
    static let headingSize: CGFloat = 14
    static let codeSize: CGFloat = 11

    static let backgroundRGB: UInt32 = 0x14182B
    static let userRGB: UInt32 = 0x2B3A67
    static let panelRGB: UInt32 = 0x0B0E1A
    static let shimmerDimRGB: UInt32 = 0x64748B

    /// One block of a turn.
    enum Block {
        /// The user's question, whole at once in a bubble at the right.
        case user(String)
        /// A heading in the bold face, which appears like a paragraph.
        case heading(String)
        /// Text that appears `tokenChars` characters a token.
        case paragraph(String)
        /// Code in a monospaced face with its stretches colored.
        case code(String)
        /// A table whose first row is its header; a row appears a token.
        case table([[String]])
        /// The animated picture.
        case gif
        /// A row of buttons.
        case buttons([String])

        /// What the status line says the agent does while the block is the last one.
        var status: String {
            switch self {
            case .user: "Reading"
            case .heading, .paragraph: "Writing"
            case .code: "Writing code"
            case .table: "Filling the table"
            case .gif: "Drawing"
            case .buttons: "Waiting"
            }
        }

        /// The tokens the block takes to appear whole.
        var tokens: Int {
            switch self {
            case .heading(let text), .paragraph(let text), .code(let text):
                (text.utf8.count + tokenChars - 1) / tokenChars
            case .table(let rows): rows.count
            case .user, .gif, .buttons: 1
            }
        }
    }

    /// One turn of the chat.
    static let turn: [Block] = [
        .user("Why does the list stutter when one row changes?"),
        .heading("What happens in a frame"),
        .paragraph(
            "The list stutters because a small change starts a large amount of work. The row "
                + "that changed shows a counter, and the counter gets a new value on every frame. "
                + "The row measures its text again, which is right, because the text is new. The "
                + "row then tells its parent that its size may have changed. The parent does not "
                + "check whether the size really changed. It measures and places every one of its "
                + "children again, the ones on the screen and the ones kept ready beyond its edges. "
                + "Each child measures its own text again, although none of that text changed. A "
                + "frame that should cost one row costs the whole list, and the cost grows with "
                + "the number of rows, not with the size of the change."),
        .paragraph(
            "The profile shows the same thing from the other side. Most of the frame goes to "
                + "text shaping, and nearly all of the shaped strings are equal to the strings of "
                + "the frame before. The cache of shaped text does not help, because the list asks "
                + "for each string at a width that differs by a fraction of a point from the width "
                + "of the last frame, so every lookup misses. Drawing takes a small part of the "
                + "frame and does not change, because the picture is almost the same."),
        .heading("Three things to change"),
        .paragraph(
            "- Give the row a fixed height. A row whose size cannot change needs no new "
                + "layout of its parent, so the work stops at the row."),
        .paragraph(
            "- Round the width that the list passes to its rows to whole points. Equal "
                + "strings then meet equal widths, and the cache of shaped text answers."),
        .paragraph(
            "- Read the counter in the draw phase where the toolkit allows it. A value "
                + "that changes only what is drawn should not start composition or layout."),
        .code(
            "// Measure only the row that changed.\n"
                + "fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n"
                + "    let row = &mut rows[dirty];\n"
                + "    if row.width == 320.0 {\n"
                + "        return row.height;\n"
                + "    }\n"
                + "    row.label = \"measured\";\n"
                + "    row.height = row.measure(320.0);\n"
                + "    row.height\n"
                + "}\n"
                + "\n"
                + "// A row of fixed height ends the pass.\n"
                + "fn place(rows: &mut [Row], first: usize) {\n"
                + "    let mut top = 0.0;\n"
                + "    for row in &mut rows[first..] {\n"
                + "        row.top = top;\n"
                + "        top = top + row.height;\n"
                + "    }\n"
                + "}\n"),
        .paragraph(
            "With the boundary in place the frame lays out one row and leaves the others "
                + "alone. I ran the list of two thousand rows before and after the change, on the "
                + "same device and at the same scroll position. These are the passes of one frame, "
                + "as the median of three hundred frames:"),
        .table([
            ["Pass", "Before", "After", "Change"],
            ["Compose", "1.92 ms", "0.31 ms", "-84%"],
            ["Measure", "4.70 ms", "0.22 ms", "-95%"],
            ["Place", "1.18 ms", "0.09 ms", "-92%"],
            ["Draw", "2.46 ms", "2.41 ms", "-2%"],
            ["Frame", "10.26 ms", "3.03 ms", "-70%"],
        ]),
        .gif,
        .paragraph(
            "Drawing is now the largest pass, and it did not move. The next step is to keep "
                + "the drawing of the rows that did not change and to draw only the row with the "
                + "counter. That needs a look at how the list records its rows, which I have not "
                + "read yet. The picture above turns while the benchmark runs; it stops when the "
                + "run ends. Tell me whether to apply the three changes, or to show the diff first."),
        .buttons(["Apply", "Show diff", "Dismiss"]),
        .paragraph("Waiting for your answer."),
    ]

    /// A token: its number from 1, the chat's last block and the tokens of that block that arrived.
    struct Token {
        let count: Int
        let block: Int
        let shown: Int
    }

    private static let starts: [Int] = {
        var total = 0
        return turn.map { block in
            defer { total += block.tokens }
            return total
        }
    }()
    private static let turnTokens = turn.reduce(0) { $0 + $1.tokens }

    /// The chat after `count` tokens, `count` at least 1.
    static func token(after count: Int) -> Token {
        let index = max(count, 1) - 1
        let within = index % turnTokens
        var block = turn.count - 1
        while starts[block] > within { block -= 1 }
        return Token(count: max(count, 1), block: index / turnTokens * turn.count + block,
                     shown: within - starts[block] + 1)
    }

    /// The turn of block `block`.
    static func turnOf(_ block: Int) -> Int { block / turn.count }

    /// The characters of `text` that `shown` tokens show. The chat's text is ASCII.
    static func shownChars(_ text: String, _ shown: Int) -> Int {
        min(shown * tokenChars, text.utf8.count)
    }

    /// The status line's text after token `token`.
    static func status(_ token: Token) -> String {
        String(format: "%@… (%06d tokens)", turn[token.block % turn.count].status, token.count % 1_000_000)
    }

    /// The mark of turn `turn` before a paragraph or a code block.
    static func turnLabel(_ turn: Int) -> String { String(format: "#%05d", turn % 100_000) }

    /// What a stretch of code is, for its color.
    enum CodeKind {
        case keyword, text, number, comment

        var rgb: UInt32 {
            switch self {
            case .keyword: 0xC084FC
            case .text: 0x86EFAC
            case .number: 0xFDBA74
            case .comment: 0x64748B
            }
        }
    }

    private static let keywords: Set<String> = ["fn", "let", "mut", "if", "for", "in", "return", "usize", "f32", "Row"]

    /// The colored stretches of `code`, in order, as ranges of its bytes. Code is ASCII.
    static func codeSpans(_ code: String) -> [(range: Range<Int>, kind: CodeKind)] {
        let bytes = Array(code.utf8)
        var spans: [(range: Range<Int>, kind: CodeKind)] = []
        func isAlpha(_ byte: UInt8) -> Bool { (65...90).contains(byte) || (97...122).contains(byte) }
        func isDigit(_ byte: UInt8) -> Bool { (48...57).contains(byte) }
        var at = 0
        while at < bytes.count {
            let start = at
            let byte = bytes[at]
            if byte == UInt8(ascii: "/") && at + 1 < bytes.count && bytes[at + 1] == UInt8(ascii: "/") {
                at = bytes[at...].firstIndex(of: UInt8(ascii: "\n")) ?? bytes.count
                spans.append((start..<at, .comment))
            } else if byte == UInt8(ascii: "\"") {
                at = bytes[(at + 1)...].firstIndex(of: UInt8(ascii: "\"")).map { $0 + 1 } ?? bytes.count
                spans.append((start..<at, .text))
            } else if isAlpha(byte) || byte == UInt8(ascii: "_") {
                while at < bytes.count && (isAlpha(bytes[at]) || isDigit(bytes[at]) || bytes[at] == UInt8(ascii: "_")) {
                    at += 1
                }
                if keywords.contains(String(decoding: bytes[start..<at], as: UTF8.self)) {
                    spans.append((start..<at, .keyword))
                }
            } else if isDigit(byte) {
                while at < bytes.count && (isDigit(bytes[at]) || bytes[at] == UInt8(ascii: ".")) { at += 1 }
                spans.append((start..<at, .number))
            } else {
                at += 1
            }
        }
        return spans
    }

    /// The frame of the animated picture that frame `frame` of the screen shows, of `frames` frames.
    static func gifFrame(_ frame: Int, _ frames: Int) -> Int { frame / gifHold % max(frames, 1) }
}

/// The tokens of the chat as they leave the data layer: a thread of its own sends them
/// through a bounded queue and waits while the screen is behind.
final class ChatFeed {
    private let lock = NSLock()
    private var queue: [ChatData.Token] = []
    private let room = DispatchSemaphore(value: ChatData.tokensPerFrame * 4)

    init() {
        let thread = Thread { [self] in
            var count = 0
            while true {
                count += 1
                room.wait()
                lock.lock()
                queue.append(ChatData.token(after: count))
                lock.unlock()
            }
        }
        thread.name = "chat-data"
        thread.start()
    }

    /// The next token, when one has arrived. The screen never waits for one.
    func take() -> ChatData.Token? {
        lock.lock()
        defer { lock.unlock() }
        guard !queue.isEmpty else { return nil }
        room.signal()
        return queue.removeFirst()
    }
}

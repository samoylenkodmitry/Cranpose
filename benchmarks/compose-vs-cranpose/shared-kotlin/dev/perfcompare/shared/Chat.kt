package dev.perfcompare.shared

import kotlin.math.abs

// The chat beside the patchwork, as `perf-data/src/chat.rs` makes it bit for
// bit: an agent's answers arrive from a data thread as tokens, several a
// frame, and the screen shows them as they come. Under the list a status
// line says what the agent does.

const val CHAT_WIDTH = 320f
const val CHAT_TOKENS_PER_FRAME = 4
const val CHAT_TOKEN_CHARS = 6
const val CHAT_GIF_HOLD = 2
const val CHAT_STATUS_HEIGHT = 28f
const val CHAT_SPINNER_SIZE = 16f
const val CHAT_SPIN_DEGREES = 12f
const val CHAT_SHIMMER_STEP = 6f
const val CHAT_SHIMMER_SPAN = 120f
const val CHAT_GIF_SIZE = 64f
const val CHAT_TEXT = 12f
const val CHAT_HEADING_TEXT = 14f
const val CHAT_CODE_TEXT = 11f
const val CHAT_BACKGROUND_ARGB = 0xFF14182B.toInt()
const val CHAT_USER_ARGB = 0xFF2B3A67.toInt()
const val CHAT_PANEL_ARGB = 0xFF0B0E1A.toInt()
const val CHAT_SHIMMER_DIM_ARGB = 0xFF64748B.toInt()
const val CHAT_SHIMMER_BRIGHT_ARGB = 0xFFFFFFFF.toInt()

/** One block of a turn. */
sealed interface ChatBlock {
    /** The user's question, whole at once in a bubble at the right. */
    data class User(val text: String) : ChatBlock

    /** A heading in the bold face, which appears like a paragraph. */
    data class Heading(val text: String) : ChatBlock

    /** Text that appears `CHAT_TOKEN_CHARS` characters a token. */
    data class Paragraph(val text: String) : ChatBlock

    /** Code in a monospaced face with its stretches colored. */
    data class Code(val text: String) : ChatBlock

    /** A table whose first row is its header; a row appears a token. */
    data class Table(val rows: List<List<String>>) : ChatBlock

    /** The animated picture. */
    data object Gif : ChatBlock

    /** A row of buttons. */
    data class Buttons(val labels: List<String>) : ChatBlock
}

/** One turn of the chat. */
val CHAT_TURN: List<ChatBlock> = listOf(
  ChatBlock.User("Why does the list stutter when one row changes?"),
  ChatBlock.Heading("What happens in a frame"),
  ChatBlock.Paragraph(
      "The list stutters because a small change starts a large amount of work. The row " +
      "that changed shows a counter, and the counter gets a new value on every frame. " +
      "The row measures its text again, which is right, because the text is new. The " +
      "row then tells its parent that its size may have changed. The parent does not " +
      "check whether the size really changed. It measures and places every one of its " +
      "children again, the ones on the screen and the ones kept ready beyond its edges. " +
      "Each child measures its own text again, although none of that text changed. A " +
      "frame that should cost one row costs the whole list, and the cost grows with " +
      "the number of rows, not with the size of the change.",
  ),
  ChatBlock.Paragraph(
      "The profile shows the same thing from the other side. Most of the frame goes to " +
      "text shaping, and nearly all of the shaped strings are equal to the strings of " +
      "the frame before. The cache of shaped text does not help, because the list asks " +
      "for each string at a width that differs by a fraction of a point from the width " +
      "of the last frame, so every lookup misses. Drawing takes a small part of the " +
      "frame and does not change, because the picture is almost the same.",
  ),
  ChatBlock.Heading("Three things to change"),
  ChatBlock.Paragraph(
      "- Give the row a fixed height. A row whose size cannot change needs no new " +
      "layout of its parent, so the work stops at the row.",
  ),
  ChatBlock.Paragraph(
      "- Round the width that the list passes to its rows to whole points. Equal " +
      "strings then meet equal widths, and the cache of shaped text answers.",
  ),
  ChatBlock.Paragraph(
      "- Read the counter in the draw phase where the toolkit allows it. A value " +
      "that changes only what is drawn should not start composition or layout.",
  ),
  ChatBlock.Code(
      "// Measure only the row that changed.\n" +
      "fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n" +
      "    let row = &mut rows[dirty];\n" +
      "    if row.width == 320.0 {\n" +
      "        return row.height;\n" +
      "    }\n" +
      "    row.label = \"measured\";\n" +
      "    row.height = row.measure(320.0);\n" +
      "    row.height\n" +
      "}\n" +
      "\n" +
      "// A row of fixed height ends the pass.\n" +
      "fn place(rows: &mut [Row], first: usize) {\n" +
      "    let mut top = 0.0;\n" +
      "    for row in &mut rows[first..] {\n" +
      "        row.top = top;\n" +
      "        top = top + row.height;\n" +
      "    }\n" +
      "}\n",
  ),
  ChatBlock.Paragraph(
      "With the boundary in place the frame lays out one row and leaves the others " +
      "alone. I ran the list of two thousand rows before and after the change, on the " +
      "same device and at the same scroll position. These are the passes of one frame, " +
      "as the median of three hundred frames:",
  ),
  ChatBlock.Table(listOf(
      listOf("Pass", "Before", "After", "Change"),
      listOf("Compose", "1.92 ms", "0.31 ms", "-84%"),
      listOf("Measure", "4.70 ms", "0.22 ms", "-95%"),
      listOf("Place", "1.18 ms", "0.09 ms", "-92%"),
      listOf("Draw", "2.46 ms", "2.41 ms", "-2%"),
      listOf("Frame", "10.26 ms", "3.03 ms", "-70%"),
    )),
  ChatBlock.Gif,
  ChatBlock.Paragraph(
      "Drawing is now the largest pass, and it did not move. The next step is to keep " +
      "the drawing of the rows that did not change and to draw only the row with the " +
      "counter. That needs a look at how the list records its rows, which I have not " +
      "read yet. The picture above turns while the benchmark runs; it stops when the " +
      "run ends. Tell me whether to apply the three changes, or to show the diff first.",
  ),
  ChatBlock.Buttons(listOf("Apply", "Show diff", "Dismiss")),
  ChatBlock.Paragraph("Waiting for your answer."),
)

private fun typedTokens(text: String): Int = (text.length + CHAT_TOKEN_CHARS - 1) / CHAT_TOKEN_CHARS

/** The tokens [block] takes to appear whole. */
fun chatTokens(block: ChatBlock): Int = when (block) {
    is ChatBlock.Heading -> typedTokens(block.text)
    is ChatBlock.Paragraph -> typedTokens(block.text)
    is ChatBlock.Code -> typedTokens(block.text)
    is ChatBlock.Table -> block.rows.size
    else -> 1
}

/** What the status line says the agent does while [block] is the last one. */
fun chatDoing(block: ChatBlock): String = when (block) {
    is ChatBlock.User -> "Reading"
    is ChatBlock.Heading, is ChatBlock.Paragraph -> "Writing"
    is ChatBlock.Code -> "Writing code"
    is ChatBlock.Table -> "Filling the table"
    ChatBlock.Gif -> "Drawing"
    is ChatBlock.Buttons -> "Waiting"
}

/** A token: its number from 1, the chat's last block and the tokens of that block that arrived. */
data class ChatToken(val count: Int, val block: Int, val shown: Int)

private val STARTS = IntArray(CHAT_TURN.size).also { starts ->
    var total = 0
    for ((index, block) in CHAT_TURN.withIndex()) {
        starts[index] = total
        total += chatTokens(block)
    }
}
private val TURN_TOKENS = CHAT_TURN.sumOf { chatTokens(it) }

/** The chat after [count] tokens, [count] at least 1. */
fun chatTokenAfter(count: Int): ChatToken {
    val index = count.coerceAtLeast(1) - 1
    val turn = index / TURN_TOKENS
    val within = index % TURN_TOKENS
    var block = CHAT_TURN.size - 1
    while (STARTS[block] > within) block--
    return ChatToken(count, turn * CHAT_TURN.size + block, within - STARTS[block] + 1)
}

/** The turn of block [block]. */
fun chatTurn(block: Int): Int = block / CHAT_TURN.size

/** The characters of [text] that [shown] tokens show; [Int.MAX_VALUE] tokens show all of it. */
fun chatShownChars(text: String, shown: Int): Int =
    if (shown >= text.length) text.length else minOf(shown * CHAT_TOKEN_CHARS, text.length)

/** The status line's text after token [token]. */
fun chatStatus(token: ChatToken): String =
    chatDoing(CHAT_TURN[token.block % CHAT_TURN.size]) + "\u2026 (" +
        (token.count % 1_000_000).toString().padStart(6, '0') + " tokens)"

/** The mark of turn [turn] before a paragraph or a code block. */
fun chatTurnLabel(turn: Int): String = "#" + (turn % 100_000).toString().padStart(5, '0')

/** What a stretch of code is, for its color. */
enum class CodeKind(val argb: Int) {
    Keyword(0xFFC084FC.toInt()),
    Text(0xFF86EFAC.toInt()),
    Number(0xFFFDBA74.toInt()),
    Comment(0xFF64748B.toInt()),
}

private val KEYWORDS = setOf("fn", "let", "mut", "if", "for", "in", "return", "usize", "f32", "Row")

/** A colored stretch of code: where it starts and ends, and its kind. */
data class CodeSpan(val start: Int, val end: Int, val kind: CodeKind)

/** The colored stretches of [code], in order. */
fun chatCodeSpans(code: String): List<CodeSpan> {
    val spans = ArrayList<CodeSpan>()
    var at = 0
    while (at < code.length) {
        val start = at
        val char = code[at]
        when {
            char == '/' && at + 1 < code.length && code[at + 1] == '/' -> {
                val end = code.indexOf('\n', at)
                at = if (end < 0) code.length else end
                spans.add(CodeSpan(start, at, CodeKind.Comment))
            }
            char == '"' -> {
                val end = code.indexOf('"', at + 1)
                at = if (end < 0) code.length else end + 1
                spans.add(CodeSpan(start, at, CodeKind.Text))
            }
            char.isLetter() || char == '_' -> {
                while (at < code.length && (code[at].isLetterOrDigit() || code[at] == '_')) at++
                if (code.substring(start, at) in KEYWORDS) spans.add(CodeSpan(start, at, CodeKind.Keyword))
            }
            char.isDigit() -> {
                while (at < code.length && (code[at].isDigit() || code[at] == '.')) at++
                spans.add(CodeSpan(start, at, CodeKind.Number))
            }
            else -> at++
        }
    }
    return spans
}

/** The frame of the animated picture that frame [frame] of the screen shows, of [frames] frames. */
fun chatGifFrame(frame: Int, frames: Int): Int = (frame / CHAT_GIF_HOLD) % frames.coerceAtLeast(1)

/** How bright the status line's gradient is [x] points from the text's start in frame [frame], 0 to 1. */
fun chatShimmer(x: Float, frame: Int): Float {
    val start = (frame * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN
    val along = ((x - start) % CHAT_SHIMMER_SPAN + CHAT_SHIMMER_SPAN) % CHAT_SHIMMER_SPAN / CHAT_SHIMMER_SPAN
    return 1f - abs(2f * along - 1f)
}

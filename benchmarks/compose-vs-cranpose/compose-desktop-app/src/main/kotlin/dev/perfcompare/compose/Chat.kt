package dev.perfcompare.compose

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.IntState
import androidx.compose.runtime.MutableIntState
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.Stable
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.loadSvgPainter
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import dev.perfcompare.shared.CHAT_BACKGROUND_ARGB
import dev.perfcompare.shared.CHAT_CODE_TEXT
import dev.perfcompare.shared.CHAT_GIF_SIZE
import dev.perfcompare.shared.CHAT_HEADING_TEXT
import dev.perfcompare.shared.CHAT_PANEL_ARGB
import dev.perfcompare.shared.CHAT_SHIMMER_BRIGHT_ARGB
import dev.perfcompare.shared.CHAT_SHIMMER_DIM_ARGB
import dev.perfcompare.shared.CHAT_SHIMMER_SPAN
import dev.perfcompare.shared.CHAT_SHIMMER_STEP
import dev.perfcompare.shared.CHAT_SPINNER_SIZE
import dev.perfcompare.shared.CHAT_SPIN_DEGREES
import dev.perfcompare.shared.CHAT_STATUS_HEIGHT
import dev.perfcompare.shared.CHAT_TEXT
import dev.perfcompare.shared.CHAT_TOKENS_PER_FRAME
import dev.perfcompare.shared.CHAT_TURN
import dev.perfcompare.shared.CHAT_USER_ARGB
import dev.perfcompare.shared.CHAT_WIDTH
import dev.perfcompare.shared.ChatBlock
import dev.perfcompare.shared.ChatToken
import dev.perfcompare.shared.CodeKind
import dev.perfcompare.shared.chatCodeSpans
import dev.perfcompare.shared.chatGifFrame
import dev.perfcompare.shared.chatShownChars
import dev.perfcompare.shared.chatStatus
import dev.perfcompare.shared.chatTokenAfter
import dev.perfcompare.shared.chatTurn
import dev.perfcompare.shared.chatTurnLabel
import java.io.File
import java.util.concurrent.ArrayBlockingQueue
import javax.imageio.ImageIO
import kotlin.concurrent.thread

// The chat beside the patchwork in Compose (`../../../../../../README.md`): an
// agent's answers arrive from a data thread as tokens and the list shows them
// as they come.
//
// In Compose's idiom the list reads only the number of blocks, so a token
// composes the last block alone, the one that reads how much of it shows. The
// status line's spinner turns in a graphics layer, which composes and draws
// nothing again; its text reads the last token and the frame in a composable
// of its own, as the animated picture reads the frame.

private val MUTED = Color(CHAT_SHIMMER_DIM_ARGB)
private val PANEL = Color(CHAT_PANEL_ARGB)
private val BUBBLE = Color(CHAT_USER_ARGB)

/**
 * What the chat shows: its blocks so far, how much of the last one and the
 * last token, which [advance] writes as tokens arrive from the data thread.
 */
@Stable
internal class ChatShown(val list: LazyListState) {
    val blocks: MutableIntState = mutableIntStateOf(0)
    val last: MutableIntState = mutableIntStateOf(0)
    val token: MutableState<ChatToken?> = mutableStateOf(null)

    // The data thread sends tokens through a bounded queue and waits while
    // the screen is behind.
    private val tokens = ArrayBlockingQueue<ChatToken>(CHAT_TOKENS_PER_FRAME * 4)

    init {
        thread(isDaemon = true, name = "chat-data") {
            var count = 0
            while (true) {
                count++
                tokens.put(chatTokenAfter(count))
            }
        }
    }

    /** Takes the frame's tokens and keeps the list at its end. The screen never waits for a token. */
    fun advance() {
        repeat(CHAT_TOKENS_PER_FRAME) {
            val arrived = tokens.poll() ?: return@repeat
            blocks.intValue = arrived.block + 1
            last.intValue = arrived.shown
            token.value = arrived
        }
        list.dispatchRawDelta(1e6f)
    }
}

@Composable
internal fun rememberChat(): ChatShown {
    val list = rememberLazyListState()
    return remember { ChatShown(list) }
}

@Composable
internal fun Chat(frame: IntState, shown: ChatShown) {
    Column(Modifier.width(CHAT_WIDTH.dp).fillMaxHeight().background(Color(CHAT_BACKGROUND_ARGB))) {
        Box(Modifier.fillMaxWidth().weight(1f)) { Blocks(frame, shown) }
        Status(frame, shown.token)
    }
}

// The list of blocks: it reads their number, so it composes again when a
// block begins and not for the tokens of a block.
@Composable
private fun Blocks(frame: IntState, shown: ChatShown) {
    val count = shown.blocks.intValue
    LazyColumn(
        state = shown.list,
        contentPadding = PaddingValues(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        userScrollEnabled = false,
    ) {
        items(count, key = { it }, contentType = { it % CHAT_TURN.size }) { index ->
            Block(index, index + 1 == count, frame, shown.last)
        }
    }
}

// Block [index] of the chat; [live] while it is the last one, the one that
// reads how much of it [shown] tokens show.
@Composable
private fun Block(index: Int, live: Boolean, frame: IntState, shown: IntState) {
    val turn = chatTurn(index)
    when (val block = CHAT_TURN[index % CHAT_TURN.size]) {
        is ChatBlock.User -> Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            BasicText(
                block.text,
                Modifier.background(BUBBLE, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 6.dp),
                style = textStyle(CHAT_TEXT, INK, false),
            )
        }
        is ChatBlock.Heading -> Typed(turn, block.text, true, live, shown)
        is ChatBlock.Paragraph -> Typed(turn, block.text, false, live, shown)
        is ChatBlock.Code -> Code(turn, block.text, live, shown)
        is ChatBlock.Table -> Table(block.rows, live, shown)
        ChatBlock.Gif -> Gif(frame)
        is ChatBlock.Buttons -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (label in block.labels) {
                BasicText(
                    label,
                    Modifier
                        .background(BUBBLE, RoundedCornerShape(6.dp))
                        .clickable { }
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    style = textStyle(CHAT_TEXT, INK, true),
                )
            }
        }
    }
}

// A paragraph, or a heading in the bold face: the turn's number, then as much
// of [text] as arrived.
@Composable
private fun Typed(turn: Int, text: String, heading: Boolean, live: Boolean, shown: IntState) {
    val chars = if (live) chatShownChars(text, shown.intValue) else text.length
    BasicText(
        chatTurnLabel(turn) + " " + text.substring(0, chars),
        Modifier.fillMaxWidth(),
        style = textStyle(if (heading) CHAT_HEADING_TEXT else CHAT_TEXT, INK, heading),
    )
}

// The whole block colored once: a comment that names the turn, then [code].
private fun highlighted(turn: Int, code: String): Pair<Int, AnnotatedString> {
    val head = "// " + chatTurnLabel(turn) + "\n"
    val text = buildAnnotatedString {
        withStyle(SpanStyle(color = Color(CodeKind.Comment.argb))) { append(head) }
        var at = 0
        for (span in chatCodeSpans(code)) {
            append(code.substring(at, span.start))
            withStyle(SpanStyle(color = Color(span.kind.argb))) { append(code.substring(span.start, span.end)) }
            at = span.end
        }
        append(code.substring(at))
    }
    return Pair(head.length, text)
}

@Composable
private fun Code(turn: Int, code: String, live: Boolean, shown: IntState) {
    val whole = remember(turn, code) { highlighted(turn, code) }
    Box(Modifier.fillMaxWidth().background(PANEL, RoundedCornerShape(6.dp)).padding(8.dp)) {
        CodeText(whole, code, live, shown)
    }
}

@Composable
private fun CodeText(whole: Pair<Int, AnnotatedString>, code: String, live: Boolean, shown: IntState) {
    val chars = if (live) chatShownChars(code, shown.intValue) else code.length
    BasicText(
        whole.second.subSequence(0, whole.first + chars),
        style = textStyle(CHAT_CODE_TEXT, INK, false).copy(fontFamily = Mono),
    )
}

@Composable
private fun Table(rows: List<List<String>>, live: Boolean, shown: IntState) {
    val count = if (live) minOf(shown.intValue, rows.size) else rows.size
    Column(Modifier.fillMaxWidth()) {
        for (index in 0 until count) {
            TableRow(rows[index], index == 0, index % 2 == 1)
        }
    }
}

@Composable
private fun TableRow(cells: List<String>, header: Boolean, shaded: Boolean) {
    Row(
        Modifier
            .fillMaxWidth()
            .background(if (header || shaded) PANEL else Color.Transparent)
            .padding(horizontal = 6.dp, vertical = 3.dp),
    ) {
        for (cell in cells) {
            BasicText(cell, Modifier.weight(1f), style = textStyle(CHAT_CODE_TEXT, INK, header))
        }
    }
}

// The animated picture's frames, decoded once for every block that shows it.
private val gifFrames: List<ImageBitmap> by lazy {
    val frames = ArrayList<ImageBitmap>()
    runCatching {
        ImageIO.createImageInputStream(File(chatAssets, "spin.gif")).use { input ->
            val reader = ImageIO.getImageReadersByFormatName("gif").next()
            reader.input = input
            for (index in 0 until reader.getNumImages(true)) {
                frames.add(reader.read(index).toComposeImageBitmap())
            }
        }
    }.onFailure { perfLog("chat picture: $it") }
    frames
}

// The animated picture: the one composable of its block that reads the frame.
@Composable
private fun Gif(frame: IntState) {
    if (gifFrames.isEmpty()) return
    Image(
        BitmapPainter(gifFrames[chatGifFrame(frame.intValue, gifFrames.size)]),
        contentDescription = null,
        Modifier.size(CHAT_GIF_SIZE.dp),
    )
}

// The status line under the list: a spinner that turns each frame and what
// the agent does, under a gradient that moves each frame.
@Composable
private fun Status(frame: IntState, token: State<ChatToken?>) {
    val density = LocalDensity.current
    val spinner: Painter? = remember(density) {
        runCatching { File(chatAssets, "progress.svg").inputStream().use { loadSvgPainter(it, density) } }
            .onFailure { perfLog("chat spinner: $it") }
            .getOrNull()
    }
    Row(
        Modifier.fillMaxWidth().height(CHAT_STATUS_HEIGHT.dp).background(PANEL).padding(horizontal = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (spinner != null) {
            Image(
                spinner,
                contentDescription = null,
                Modifier.size(CHAT_SPINNER_SIZE.dp).graphicsLayer { rotationZ = frame.intValue * CHAT_SPIN_DEGREES },
            )
        }
        StatusText(frame, token)
    }
}

@Composable
private fun StatusText(frame: IntState, token: State<ChatToken?>) {
    val said = token.value?.let(::chatStatus) ?: return
    val (start, span) = with(LocalDensity.current) {
        Pair(((frame.intValue * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN).dp.toPx(), CHAT_SHIMMER_SPAN.dp.toPx())
    }
    val shimmer = Brush.linearGradient(
        listOf(MUTED, Color(CHAT_SHIMMER_BRIGHT_ARGB), MUTED),
        start = Offset(start, 0f),
        end = Offset(start + span, 0f),
        tileMode = TileMode.Repeated,
    )
    BasicText(said, style = textStyle(CHAT_TEXT, INK, false).copy(brush = shimmer))
}

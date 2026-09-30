package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import java.io.File
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

// The trading workspace of gpui-fast's `gpui_perf` showcase
// (github.com/longbridge/gpui-fast, crates/gpui_perf/src/showcase), the most
// complex screen it measures, element for element as the Cranpose app draws
// it: the showcase's frame around a docked window of market panels that a
// timer streams quotes into. See the Cranpose app's `screens/workspace.rs`.

private const val SYMBOLS = 200
private const val HIDDEN_PANELS = 16
private const val ROW_HEIGHT = 32f
private const val FIRST_SCREEN = 40
private const val SPARK_POINTS = 40
private const val TRADES = 80
private const val BOOK_LEVELS = 10
private const val CANDLES = 120
private const val QUOTES_PER_CANDLE = 40
private const val SELECTED = 7
private const val STREAM_EVERY_MS = 16L
private const val SCROLL_STEP = 32f
private const val WINDOW_WIDTH = 1280f
private const val WINDOW_HEIGHT = 820f

enum class WorkspaceMode(val quotesPerTick: Int) {
    Quotes(16), Scroll(8), Hover(8);

    companion object {
        fun fromName(name: String?): WorkspaceMode = when (name) {
            "scroll" -> Scroll
            "hover" -> Hover
            else -> Quotes
        }
    }
}

// The showcase's light theme.
private val Background = Color(0xFFFFFFFF)
private val Foreground = Color(0xFF0A0A0A)
private val Muted = Color(0xFFF4F4F5)
private val MutedForeground = Color(0xFF71717A)
private val BorderColor = Color(0xFFE4E4E7)
private val InputColor = Color(0xFFD4D4D8)
private val SidebarColor = Color(0xFFFAFAFA)
private val SidebarAccent = Color(0xFFECECEE)
private val Secondary = Color(0xFFF4F4F5)
private val Primary = Color(0xFF18181B)
private val PrimaryForeground = Color(0xFFFAFAFA)
private val Stripe = Color(0.5f, 0.5f, 0.5f, 0.03f)
private val Success = Color(0xFF15803D)
private val Danger = Color(0xFFDC2626)
private val Series = listOf(Color(0xFFCA8A04), Color(0xFF9333EA), Color(0xFF0284C7))
private const val RADIUS_SM = 4f
private const val RADIUS = 6f

// The device's Roboto faces, the same two files the Cranpose app loads: the
// showcase's medium weight draws regular, its semibold bold.
private val WorkspaceFont = FontFamily(
    Font(File("/system/fonts/Roboto-Regular.ttf"), FontWeight.Normal),
    Font(File("/system/fonts/Roboto-Bold.ttf"), FontWeight.Bold),
)

private fun changeColor(change: Double) = if (change >= 0) Success else Danger

/** Text in the showcase's type scale: `size` sp with Tailwind's line height, figures tabular. */
private fun style(size: Float, color: Color, bold: Boolean = false): TextStyle {
    val line = when {
        size <= 12f -> 16f
        size <= 14f -> 20f
        size <= 20f -> 28f
        else -> 32f
    }
    return TextStyle(
        color = color,
        fontSize = size.sp,
        lineHeight = line.sp,
        fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal,
        fontFamily = WorkspaceFont,
        fontFeatureSettings = "tnum",
    )
}

private fun xs(color: Color) = style(12f, color)
private fun sm(color: Color) = style(14f, color)

@Composable
private fun Label(text: String, modifier: Modifier = Modifier, style: TextStyle) {
    BasicText(text, modifier, style, overflow = TextOverflow.Clip, softWrap = false, maxLines = 1)
}

/** Draws a hairline along each side `sides` names: left, top, right, bottom. */
private fun Modifier.hairlines(left: Boolean = false, top: Boolean = false, right: Boolean = false, bottom: Boolean = false) =
    drawBehind {
        val line = 1.dp.toPx()
        if (left) drawRect(BorderColor, Offset.Zero, Size(line, size.height))
        if (top) drawRect(BorderColor, Offset.Zero, Size(size.width, line))
        if (right) drawRect(BorderColor, Offset(size.width - line, 0f), Size(line, size.height))
        if (bottom) drawRect(BorderColor, Offset(0f, size.height - line), Size(size.width, line))
    }

// Number formats, as the showcase writes them.

private fun grouped(value: Double, decimals: Int): String {
    val text = String.format(java.util.Locale.ROOT, "%.${decimals}f", abs(value))
    val dot = text.indexOf('.')
    val whole = if (dot < 0) text else text.substring(0, dot)
    val fraction = if (dot < 0) "" else text.substring(dot + 1)
    val out = StringBuilder(text.length + whole.length / 3 + 1)
    if (value < 0 && text.any { it in '1'..'9' }) out.append('-')
    for ((ix, digit) in whole.withIndex()) {
        if (ix > 0 && (whole.length - ix) % 3 == 0) out.append(',')
        out.append(digit)
    }
    if (fraction.isNotEmpty()) out.append('.').append(fraction)
    return out.toString()
}

private fun price(value: Double) = grouped(value, 3)
private fun signed(value: Double): String = grouped(value, 3).let { if (it.startsWith("-")) it else "+$it" }
private fun percent(value: Double) = String.format(java.util.Locale.ROOT, "%+.2f%%", value)
private fun fixed2(value: Double) = String.format(java.util.Locale.ROOT, "%.2f", value)
private fun amount(value: Double): String = when {
    value >= 1e9 -> String.format(java.util.Locale.ROOT, "%.2fB", value / 1e9)
    value >= 1e6 -> String.format(java.util.Locale.ROOT, "%.2fM", value / 1e6)
    value >= 1e3 -> String.format(java.util.Locale.ROOT, "%.2fK", value / 1e3)
    else -> String.format(java.util.Locale.ROOT, "%.0f", value)
}

// The market: every symbol's quote as a state of its own, and the state each
// quote panel draws from.

private data class SymbolQuote(
    val last: Double,
    val prevClose: Double,
    val open: Double,
    val high: Double,
    val low: Double,
    val volume: Long,
    val turnover: Double,
    val preMarket: Double,
    val high52: Double,
    val low52: Double,
    val floatShares: Double,
    val averageVolume: Double,
)

private class QuoteEvent(val symbol: Int, val last: Double, val volume: Long)
private class Candle(var open: Double, var high: Double, var low: Double, var close: Double, var volume: Double)
private data class Trade(val time: Int, val price: Double, val size: Long, val buy: Boolean)

private val NameWords = listOf(
    "Pacific", "Golden", "Northern", "Silver", "Eastern", "United", "Harbor", "Summit",
    "Crystal", "Pioneer", "Atlas", "Meridian", "Evergreen", "Horizon", "Sterling", "Orion",
)
private val NameKinds = listOf(
    "Technology", "Energy", "Holdings", "Motors", "Semiconductor", "Pharmaceuticals", "Bank",
    "Networks", "Materials", "Logistics",
)
private val Markets = listOf("US", "HK", "US", "SH", "US", "HK", "SG")

private fun initialQuote(ix: Int): SymbolQuote {
    val base = 5.0 + (ix * 37 % 1_400) + (ix % 7) * 0.13
    val prevClose = base * (1.0 + ((ix % 11) - 5.0) / 300.0)
    val volume = 1_000_000L + (ix.toLong() * 7_919_113L) % 90_000_000L
    return SymbolQuote(
        last = base,
        prevClose = prevClose,
        open = prevClose * 1.002,
        high = base * 1.02,
        low = base * 0.98,
        volume = volume,
        turnover = volume * base,
        preMarket = prevClose * (1.0 + ((ix % 9) - 4.0) / 400.0),
        high52 = base * 1.6,
        low52 = base * 0.55,
        floatShares = 2e8 + (ix * 3.7e7) % 9e9,
        averageVolume = volume * (0.6 + (ix % 5) * 0.2),
    )
}

private class Market {
    val quotes: List<MutableState<SymbolQuote>> = List(SYMBOLS) { mutableStateOf(initialQuote(it)) }
    val names: List<String> = List(SYMBOLS) { "${NameWords[it % NameWords.size]} ${NameKinds[it * 7 % NameKinds.size]}" }
    val codes: List<String> = List(SYMBOLS) { ix ->
        fun letter(n: Int) = 'A' + (n % 26)
        when (Markets[ix % Markets.size]) {
            "HK" -> String.format(java.util.Locale.ROOT, "%05d", ix * 97 % 9_999)
            "SH" -> String.format(java.util.Locale.ROOT, "6%05d", ix * 131 % 99_999)
            else -> buildString {
                append(letter(ix * 7 + 3)); append(letter(ix * 11 + 5)); append(letter(ix / 26 + ix * 3))
                if (ix % 3 != 0) append(letter(ix * 5))
            }
        }
    }
    /** Each symbol's day before its last point, which follows its quote. */
    val sparks: List<FloatArray> = List(SYMBOLS) { ix ->
        val prevClose = quotes[ix].value.prevClose
        FloatArray(SPARK_POINTS) { point ->
            val wave = ((point * 7 + ix * 3) % 17) - 8.0
            (prevClose * (1.0 + wave / 400.0)).toFloat()
        }
    }
    val candles: ArrayDeque<Candle>
    var chartQuotes = 0
    val chart = mutableIntStateOf(0)
    val book = mutableStateOf(quotes[SELECTED].value.last to 0)
    val trades: ArrayDeque<Trade>
    val tape = mutableIntStateOf(0)
    val buckets = mutableStateOf(listOf(4.2e8, 2.9e8, 1.1e8, 1.3e8, 2.4e8, 3.8e8))
    val status = mutableIntStateOf(0)
    val hidden = IntArray(HIDDEN_PANELS)
    var ticks = 0

    init {
        val selected = quotes[SELECTED].value
        var close = selected.prevClose
        candles = ArrayDeque((0 until CANDLES).map { ix ->
            val open = close
            val step = ((ix * 7 % 13) - 6.0) / 300.0
            close = open * (1.0 + step)
            val reach = ((ix * 5 % 7) + 1.0) / 800.0
            Candle(open, max(open, close) * (1 + reach), min(open, close) * (1 - reach), close, 2e6 + ((ix * 7_919) % 5_000) * 1e3)
        })
        trades = ArrayDeque((0 until TRADES).map { ix ->
            Trade(37_800 - ix * 3, selected.last + ((ix * 7 % 11) - 5.0) * 0.01, 100L * (1 + (ix.toLong() * 37) % 60), ix % 3 != 0)
        })
    }

    /** One tick of the stream: each quote updates its symbol and goes out to every panel. */
    fun tick(quotesPerTick: Int) {
        val tick = ++ticks
        for (i in 0 until quotesPerTick) {
            val symbol = quoteSymbol(tick, i)
            val state = quotes[symbol]
            val quote = state.value
            val step = ((tick * 31 + symbol * 17) % 21) - 10.0
            val last = max(quote.last * (1.0 + step / 2_000.0), 0.01)
            val traded = 100L + (tick % 9) * 100L
            val updated = quote.copy(
                last = last,
                high = max(quote.high, last),
                low = min(quote.low, last),
                volume = quote.volume + traded,
                turnover = quote.turnover + traded * last,
            )
            state.value = updated
            emit(QuoteEvent(symbol, updated.last, updated.volume))
        }
        if (tick % 30 == 0) status.intValue += 1
    }

    /** Hands a quote to every panel subscribed to the feed. */
    private fun emit(event: QuoteEvent) {
        for (panel in hidden.indices) hidden[panel] += 1
        if (event.symbol != SELECTED) return
        chartQuotes += 1
        if (chartQuotes % QUOTES_PER_CANDLE == 0) {
            val close = candles.last().close
            candles.removeFirst()
            candles.addLast(Candle(close, close, close, close, 0.0))
        }
        candles.last().apply {
            close = event.last
            high = max(high, event.last)
            low = min(low, event.last)
            volume += (event.volume % 1_000) * 100.0
        }
        chart.intValue += 1
        book.value = event.last to book.value.second + 1
        val newest = trades.first()
        trades.addFirst(Trade(newest.time + 1, event.last, 100L * (1 + event.volume % 60), event.last >= newest.price))
        while (trades.size > TRADES) trades.removeLast()
        tape.intValue += 1
        buckets.value = buckets.value.toMutableList().also { it[(event.volume % 6).toInt()] += (event.volume % 1_000) * 1e3 }
    }
}

private fun quoteSymbol(tick: Int, i: Int): Int = when {
    i == 0 && tick % 2 == 0 -> SELECTED
    i in 1..4 -> (tick * 5 + i * 11) % FIRST_SCREEN
    else -> (tick * 7 + i * 13) % SYMBOLS
}

/**
 * The workspace at the showcase window's size in dp: a screen of another
 * width draws it at the density that makes it span that width, so every
 * device lays out the same 1280 × 820 dp.
 */
@Composable
fun WorkspaceFrame(mode: WorkspaceMode) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // gpui_perf's window keeps its size; a narrower screen shows all of
        // it scaled down, as the Cranpose app does.
        val scale = (maxWidth.value / WINDOW_WIDTH).coerceAtMost(1f)
        val canvas = Modifier
            .wrapContentSize(Alignment.TopStart, unbounded = true)
            .size(WINDOW_WIDTH.dp, WINDOW_HEIGHT.dp)
        Box(
            if (scale < 1f) {
                canvas.graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    transformOrigin = TransformOrigin(0f, 0f)
                }
            } else {
                canvas
            },
        ) { WorkspaceScreen(mode) }
    }
}

@Composable
private fun WorkspaceScreen(mode: WorkspaceMode) {
    val market = remember { Market() }
    val watchlist = rememberLazyListState()
    val fps = remember { mutableFloatStateOf(0f) }
    val step = with(LocalDensity.current) { SCROLL_STEP.dp.toPx() }
    LaunchedEffect(mode) {
        while (isActive) {
            delay(STREAM_EVERY_MS)
            market.tick(mode.quotesPerTick)
        }
    }
    LaunchedEffect(mode) {
        var direction = 1f
        var windowStart = withFrameNanos { it }
        var windowFrames = 0
        while (isActive) {
            val now = withFrameNanos { it }
            windowFrames += 1
            // The frame-stats bar samples twice a second, as the showcase's does.
            if (now - windowStart >= 500_000_000L) {
                fps.floatValue = windowFrames * 1e9f / (now - windowStart)
                windowStart = now
                windowFrames = 0
            }
            if (mode == WorkspaceMode.Scroll) {
                if (!watchlist.canScrollForward) direction = -1f
                else if (!watchlist.canScrollBackward) direction = 1f
                watchlist.dispatchRawDelta(step * direction)
            }
        }
    }
    Column(Modifier.fillMaxSize().background(Background)) {
        ShowcaseToolbar()
        Row(Modifier.fillMaxWidth().weight(1f)) {
            GallerySidebar()
            Column(Modifier.weight(1f).fillMaxHeight()) {
                PageHeader()
                Workspace(market, watchlist)
            }
        }
        FrameStats(fps.floatValue)
    }
}

@Composable
private fun ShowcaseToolbar() {
    Row(
        Modifier.fillMaxWidth().height(40.dp).hairlines(bottom = true).padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.height(24.dp).background(Success, RoundedCornerShape(RADIUS.dp)).padding(horizontal = 8.dp),
            contentAlignment = Alignment.Center,
        ) { Label("compose", style = style(14f, Color.White)) }
        Spacer(Modifier.weight(1f))
        Label("Auto-scroll", style = sm(MutedForeground))
        Row(
            Modifier.background(Muted, RoundedCornerShape(RADIUS.dp)).padding(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            listOf("Off", "Sidebar", "Page", "Table", "List", "Watchlist").forEachIndexed { ix, label ->
                val selected = ix == 0
                Box(
                    Modifier.height(24.dp)
                        .then(if (selected) Modifier.background(Background, RoundedCornerShape(RADIUS_SM.dp)) else Modifier)
                        .padding(horizontal = 8.dp),
                    contentAlignment = Alignment.Center,
                ) { Label(label, style = sm(if (selected) Foreground else MutedForeground)) }
            }
        }
        Box(Modifier.size(1.dp, 16.dp).background(BorderColor))
        for ((label, on) in listOf("Refresh data" to false, "Stream quotes" to true, "Retained views" to true)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier.size(28.dp, 16.dp).background(if (on) Primary else InputColor, RoundedCornerShape(8.dp)),
                    contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart,
                ) {
                    Box(Modifier.padding(2.dp).size(12.dp).background(Background, RoundedCornerShape(6.dp)))
                }
                Label(label, style = sm(Foreground))
            }
        }
    }
}

private val Groups = listOf(
    "Getting started" to 6, "Inputs" to 32, "Buttons" to 18, "Data display" to 36, "Feedback" to 22,
    "Navigation" to 24, "Overlays" to 20, "Layout" to 22, "Forms" to 26, "Charts" to 16, "Media" to 12,
    "Applications" to 9,
)
private val PageWords = listOf(
    "Accordion", "Badge", "Calendar", "Dropdown", "Editor", "Form", "Grid", "Hover card", "Input",
    "Kbd", "Label", "Menu",
)

@Composable
private fun GallerySidebar() {
    val scroll = rememberScrollState()
    Column(Modifier.width(256.dp).fillMaxHeight().background(SidebarColor).hairlines(right = true)) {
        Column(
            Modifier.fillMaxWidth().weight(1f).verticalScroll(scroll)
                .padding(start = 8.dp, end = 8.dp, bottom = 16.dp),
        ) {
            var page = 0
            for ((title, pages) in Groups) {
                Label(
                    title,
                    Modifier.padding(start = 8.dp, top = 16.dp, end = 8.dp, bottom = 4.dp),
                    style(12f, MutedForeground),
                )
                repeat(pages) {
                    val selected = page == 241
                    val name = if (selected) "Trading workspace" else "${PageWords[page % PageWords.size]} ${page / 12 + 1}"
                    Box(
                        Modifier.fillMaxWidth().height(28.dp)
                            .then(if (selected) Modifier.background(SidebarAccent, RoundedCornerShape(RADIUS.dp)) else Modifier)
                            .padding(horizontal = 8.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) { Label(name, style = style(14f, Foreground)) }
                    page += 1
                }
            }
        }
        Label(
            "3 unread",
            Modifier.fillMaxWidth().hairlines(top = true).padding(horizontal = 16.dp, vertical = 8.dp),
            xs(MutedForeground),
        )
    }
}

@Composable
private fun PageHeader() {
    Column(
        Modifier.fillMaxWidth().hairlines(bottom = true).padding(horizontal = 24.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Label("Trading workspace", style = style(20f, Foreground, bold = true))
        Label("Docked market panels, every one subscribed to a feed of streaming quotes.", style = sm(MutedForeground))
    }
}

@Composable
private fun FrameStats(fps: Float) {
    Row(
        Modifier.fillMaxWidth().height(24.dp).hairlines(top = true).padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label("${fps.toInt()} fps", style = xs(MutedForeground))
        Label("frames drawn by compose", style = xs(MutedForeground))
    }
}

@Composable
private fun LetterIcon(letter: String, side: Float, fill: Color, color: Color, round: Boolean = false) {
    Box(
        Modifier.size(side.dp).background(fill, RoundedCornerShape(if (round) (side / 2).dp else RADIUS_SM.dp)),
        contentAlignment = Alignment.Center,
    ) { Label(letter, style = style(side * 0.6f, color, bold = true)) }
}

@Composable
private fun Chip(label: String, selected: Boolean, vertical: Float, textSize: Float) {
    Box(
        Modifier.then(if (selected) Modifier.background(Secondary, RoundedCornerShape(RADIUS_SM.dp)) else Modifier)
            .padding(horizontal = 8.dp, vertical = vertical.dp),
    ) { Label(label, style = style(textSize, if (selected) Foreground else MutedForeground)) }
}

@Composable
private fun Workspace(market: Market, watchlist: LazyListState) {
    Column(Modifier.fillMaxSize()) {
        WorkspaceToolbar()
        Row(Modifier.fillMaxWidth().weight(1f)) {
            IconSidebar()
            Dock(market, watchlist)
        }
        StatusBar(market.status.intValue)
    }
}

@Composable
private fun WorkspaceToolbar() {
    Row(
        Modifier.fillMaxWidth().height(40.dp).hairlines(bottom = true).padding(horizontal = 12.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.padding(end = 8.dp)) { LetterIcon("T", 24f, Primary, PrimaryForeground) }
            listOf("Watchlist", "Markets", "Portfolio", "Trade", "Screener", "News", "Community")
                .forEachIndexed { ix, title -> Chip(title, ix == 0, 4f, 14f) }
        }
        Row(
            Modifier.width(288.dp).height(28.dp)
                .border(1.dp, Foreground, RoundedCornerShape(RADIUS.dp))
                .padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) { Label("Search symbols", style = sm(MutedForeground)) }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(8.dp).background(Success, RoundedCornerShape(4.dp)))
                Label("US Market Open", style = xs(Foreground))
            }
            LetterIcon("!", 20f, Secondary, MutedForeground)
            LetterIcon("*", 20f, Secondary, MutedForeground)
            Label("Account A/C(1637)", style = xs(MutedForeground))
            LetterIcon("J", 24f, Series[1], Color.White, round = true)
        }
    }
}

@Composable
private fun IconSidebar() {
    Column(
        Modifier.width(48.dp).fillMaxHeight().background(SidebarColor).hairlines(right = true).padding(vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        listOf("W", "M", "P", "T", "S", "N", "C", "A", "L", "O").forEachIndexed { ix, letter ->
            Box(
                Modifier.size(36.dp).then(if (ix == 0) Modifier.background(SidebarAccent, RoundedCornerShape(RADIUS.dp)) else Modifier),
                contentAlignment = Alignment.Center,
            ) { LetterIcon(letter, 18f, Color.Transparent, if (ix == 0) Foreground else MutedForeground) }
        }
        Spacer(Modifier.weight(1f))
        LetterIcon("?", 18f, Color.Transparent, MutedForeground)
    }
}

@Composable
private fun androidx.compose.foundation.layout.RowScope.Dock(market: Market, watchlist: LazyListState) {
    Row(Modifier.weight(1f).fillMaxHeight()) {
        Column(Modifier.weight(1f).fillMaxHeight()) {
            Panel(1f, right = true, bottom = false, tabs = listOf("Watchlist", "Positions")) { Watchlist(market, watchlist) }
        }
        Column(Modifier.width(460.dp).fillMaxHeight()) {
            Panel(1f, right = true, bottom = true, tabs = listOf("Quote", "Profile")) { QuoteDetail(market) }
            Panel(1.2f, right = true, bottom = false, tabs = listOf("Candlestick", "Intraday")) { Chart(market) }
        }
        Column(Modifier.width(380.dp).fillMaxHeight()) {
            Panel(1f, right = false, bottom = true, tabs = listOf("Order Book")) { OrderBook(market) }
            Panel(1.6f, right = false, bottom = true, tabs = listOf("Time & Sales", "News")) { TimeAndSales(market) }
            Panel(0.7f, right = false, bottom = false, tabs = listOf("Statistics")) { TradeStats(market) }
        }
    }
}

/** A dock slot: a tab bar over its active panel. */
@Composable
private fun androidx.compose.foundation.layout.ColumnScope.Panel(
    grow: Float,
    right: Boolean,
    bottom: Boolean,
    tabs: List<String>,
    content: @Composable () -> Unit,
) {
    Column(Modifier.fillMaxWidth().weight(grow).hairlines(right = right, bottom = bottom).clipToBounds()) {
        Row(
            Modifier.fillMaxWidth().height(32.dp).background(Muted).hairlines(bottom = true),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            tabs.forEachIndexed { ix, title -> WorkspaceTab(title, active = ix == 0) }
        }
        Box(Modifier.fillMaxWidth().weight(1f).background(Background).clipToBounds()) { content() }
    }
}

/**
 * One tab in a dock panel's strip. The active tab has no hover style; an
 * inactive tab tints to [BorderColor] (gpui's `secondary_hover`) while the
 * pointer rests over it.
 */
@Composable
private fun WorkspaceTab(title: String, active: Boolean) {
    val interaction = remember { MutableInteractionSource() }
    val isHovered by interaction.collectIsHoveredAsState()
    Box(
        Modifier.fillMaxHeight()
            .hoverable(interaction, enabled = !active)
            .then(
                when {
                    active -> Modifier.background(Background)
                    isHovered -> Modifier.background(BorderColor)
                    else -> Modifier
                },
            )
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) { Label(title, style = sm(if (active) Foreground else MutedForeground)) }
}

private class Column_(val title: String, val width: Float, val numeric: Boolean)

private val Columns = listOf(
    Column_("Symbol", 92f, false), Column_("Name", 200f, false), Column_("Price", 84f, true),
    Column_("% Chg", 76f, true), Column_("Chg", 76f, true), Column_("Volume", 128f, true),
    Column_("Turnover", 84f, true), Column_("Turnover %", 88f, true), Column_("Vol Ratio", 76f, true),
    Column_("High", 84f, true), Column_("Low", 84f, true), Column_("Pre-Mkt", 84f, true),
    Column_("Pre-Mkt %", 84f, true), Column_("52wk Range", 108f, false), Column_("Trend", 100f, false),
)
private const val SORTED_BY = 3
private const val TABLE_WIDTH = 1448f + 8f

/** One cell of a watchlist row: its column's width, padded, numbers trailing. */
@Composable
private fun Cell(column: Int, gap: Float, content: @Composable () -> Unit) {
    val spec = Columns[column]
    Row(
        Modifier.width(spec.width.dp).fillMaxHeight().padding(horizontal = 8.dp).clipToBounds(),
        horizontalArrangement = if (spec.numeric) Arrangement.spacedBy(gap.dp, Alignment.End) else Arrangement.spacedBy(gap.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) { content() }
}

@Composable
private fun Watchlist(market: Market, state: LazyListState) {
    val scroll = rememberScrollState()
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().height(32.dp).padding(horizontal = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            listOf("All 200", "US Stocks", "HK Stocks", "China A", "Singapore", "Holdings", "ETFs")
                .forEachIndexed { ix, group -> Chip(group, ix == 0, 2f, 12f) }
            Spacer(Modifier.weight(1f))
            Label("Sorted by % Chg", Modifier.padding(end = 8.dp), xs(MutedForeground))
        }
        Column(Modifier.fillMaxWidth().weight(1f).horizontalScroll(scroll, enabled = false)) {
            WatchlistHeader()
            LazyColumn(Modifier.width(TABLE_WIDTH.dp).weight(1f), state) {
                items(SYMBOLS, key = { it }, contentType = { 0 }) { ix -> WatchlistRow(market, ix) }
            }
        }
    }
}

@Composable
private fun WatchlistHeader() {
    Row(
        Modifier.width(TABLE_WIDTH.dp).height(32.dp).background(Muted).hairlines(top = true, bottom = true)
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Columns.forEachIndexed { column, spec ->
            Cell(column, 4f) {
                Label(spec.title, style = style(12f, MutedForeground))
                if (spec.numeric) SortArrows(column == SORTED_BY)
            }
        }
    }
}

/** The two small triangles of a sortable column, the one sorted by filled. */
@Composable
private fun SortArrows(sortedDown: Boolean) {
    Box(
        Modifier.size(6.dp, 12.dp).drawBehind {
            val w = size.width
            val y = size.height / 2
            val unit = 1.dp.toPx()
            for (up in listOf(true, false)) {
                val base = y + if (up) -unit else unit
                val tip = if (up) base - 4 * unit else base + 4 * unit
                val path = Path().apply { moveTo(0f, base); lineTo(w, base); lineTo(w / 2, tip); close() }
                drawPath(path, if (!up && sortedDown) Foreground else BorderColor)
            }
        },
    )
}

@Composable
private fun WatchlistRow(market: Market, ix: Int) {
    val quote = market.quotes[ix].value
    val change = quote.last - quote.prevClose
    val color = changeColor(change)
    val preChange = quote.preMarket - quote.prevClose
    val preColor = changeColor(preChange)
    val volume = quote.volume.toDouble()
    val numbers = listOf(
        price(quote.last) to color,
        percent(change / quote.prevClose * 100) to color,
        signed(change) to color,
        "${amount(volume)} shares" to null,
        amount(quote.turnover) to null,
        "${fixed2(volume / quote.floatShares * 100)}%" to null,
        fixed2(volume / quote.averageVolume) to null,
        price(quote.high) to null,
        price(quote.low) to null,
        price(quote.preMarket) to preColor,
        percent(preChange / quote.prevClose * 100) to preColor,
    )
    val range = ((quote.last - quote.low52) / (quote.high52 - quote.low52)).coerceIn(0.0, 1.0).toFloat()
    val spark = market.sparks[ix].copyOf().also { it[SPARK_POINTS - 1] = quote.last.toFloat() }
    val baseline = quote.prevClose.toFloat()
    val interaction = remember { MutableInteractionSource() }
    val isHovered by interaction.collectIsHoveredAsState()
    Row(
        Modifier.width(TABLE_WIDTH.dp).height(ROW_HEIGHT.dp)
            .hoverable(interaction)
            .drawBehind {
                if (isHovered) drawRect(Muted) else if (ix % 2 == 1) drawRect(Stripe)
                drawRect(BorderColor, Offset(0f, size.height - 1.dp.toPx()), Size(size.width, 1.dp.toPx()))
            }
            .padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Cell(0, 4f) {
            Box(Modifier.background(Secondary, RoundedCornerShape(RADIUS_SM.dp)).padding(horizontal = 2.dp)) {
                Label(Markets[ix % Markets.size], style = xs(MutedForeground))
            }
            Label(market.codes[ix], style = style(14f, Foreground))
        }
        Cell(1, 8f) {
            LetterIcon(listOf("A", "B", "C", "D", "E", "F", "G", "H")[ix % 8], 18f, Secondary, MutedForeground, round = true)
            Label(market.names[ix], Modifier.weight(1f), sm(Foreground))
            for ((shown, letter) in listOf((ix % 4 == 0) to "H", (ix % 3 == 1) to "P", (ix % 5 == 2) to "O")) {
                if (shown) {
                    Box(
                        Modifier.size(16.dp).border(1.dp, InputColor, RoundedCornerShape(RADIUS_SM.dp)),
                        contentAlignment = Alignment.Center,
                    ) { Label(letter, style = xs(MutedForeground)) }
                }
            }
        }
        numbers.forEachIndexed { offset, (text, textColor) ->
            Cell(offset + 2, 0f) { Label(text, style = sm(textColor ?: Foreground)) }
        }
        Cell(13, 0f) {
            Box(
                Modifier.fillMaxWidth().height(8.dp).drawBehind {
                    val bar = 4.dp.toPx()
                    val top = (size.height - bar) / 2
                    drawRoundRect(Muted, Offset(0f, top), Size(size.width, bar), CornerRadius(bar / 2))
                    drawRoundRect(InputColor, Offset(0f, top), Size(size.width * range, bar), CornerRadius(bar / 2))
                    drawRect(Foreground, Offset(size.width * range, 0f), Size(2.dp.toPx(), size.height))
                },
            )
        }
        Cell(14, 0f) {
            Box(Modifier.fillMaxWidth().height(20.dp).drawBehind { drawSpark(spark, baseline, color) })
        }
    }
}

/** A symbol's day as a line over a faint fill. */
private fun DrawScope.drawSpark(points: FloatArray, baseline: Float, color: Color) {
    var low = baseline
    var high = baseline
    for (point in points) { low = min(low, point); high = max(high, point) }
    val span = max(high - low, 0.0001f)
    val step = size.width / (points.size - 1)
    fun y(value: Float) = size.height * ((high - value) / span)
    val area = Path().apply { moveTo(0f, size.height) }
    val line = Path()
    points.forEachIndexed { ix, value ->
        area.lineTo(step * ix, y(value))
        if (ix == 0) line.moveTo(0f, y(value)) else line.lineTo(step * ix, y(value))
    }
    area.lineTo(size.width, size.height)
    area.close()
    drawPath(area, color.copy(alpha = 0.12f))
    drawPath(line, color, style = Stroke(1.dp.toPx()))
}

@Composable
private fun QuoteDetail(market: Market) {
    val quote = market.quotes[SELECTED].value
    val change = quote.last - quote.prevClose
    val color = changeColor(change)
    val preChange = quote.preMarket - quote.prevClose
    val volume = quote.volume.toDouble()
    val shares = quote.floatShares * 1.08
    val stats = listOf(
        "Open" to price(quote.open), "Prev. Close" to price(quote.prevClose),
        "High" to price(quote.high), "Low" to price(quote.low),
        "Volume" to "${amount(volume)} shares", "Turnover" to amount(quote.turnover),
        "52wk High" to price(quote.high52), "52wk Low" to price(quote.low52),
        "Mkt Cap" to amount(shares * quote.last), "Float Cap" to amount(quote.floatShares * quote.last),
        "Shares" to amount(shares), "Float Shares" to amount(quote.floatShares),
        "P/E (TTM)" to fixed2(quote.last / 8.83), "P/E (Static)" to fixed2(quote.last / 7.91),
        "P/B" to fixed2(quote.last / 21.4), "Dividend (TTM)" to "1.060",
        "Div. Yield" to "${fixed2(1.06 / quote.last * 100)}%", "Turnover %" to "${fixed2(volume / quote.floatShares * 100)}%",
        "Vol Ratio" to fixed2(volume / quote.averageVolume),
        "Amplitude" to "${fixed2((quote.high - quote.low) / quote.prevClose * 100)}%",
        "Avg Price" to price(quote.turnover / volume), "Bid/Ask" to "${fixed2(50 + change * 3)}%",
        "Lot Size" to "100", "Min Tick" to "0.010",
    )
    Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Label(market.codes[SELECTED], style = style(14f, Foreground, bold = true))
            Label(market.names[SELECTED], style = xs(MutedForeground))
            Box(Modifier.background(Success.copy(alpha = 0.15f), RoundedCornerShape(RADIUS_SM.dp)).padding(horizontal = 4.dp)) {
                Label("Trading", style = xs(Success))
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Label(price(quote.last), style = style(24f, color, bold = true))
            Label(signed(change), style = sm(color))
            Label(percent(change / quote.prevClose * 100), style = sm(color))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            val pre = changeColor(preChange)
            Label("Pre-market", style = xs(MutedForeground))
            Label(price(quote.preMarket), style = xs(pre))
            Label(percent(preChange / quote.prevClose * 100), style = xs(pre))
            Label("04:12 EST", style = xs(MutedForeground))
        }
        FlowRow(Modifier.fillMaxWidth()) {
            stats.forEachIndexed { ix, (label, value) ->
                Row(
                    Modifier.fillMaxWidth(0.5f).padding(
                        start = if (ix % 2 == 1) 12.dp else 0.dp,
                        top = 2.dp,
                        end = if (ix % 2 == 0) 12.dp else 0.dp,
                        bottom = 2.dp,
                    ),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Label(label, style = xs(MutedForeground))
                    Label(value, style = xs(Foreground))
                }
            }
        }
    }
}

private val Averages = listOf(5, 10, 20)
private const val PRICE_AXIS = 64f
private const val TIME_AXIS = 18f

@Composable
private fun Chart(market: Market) {
    market.chart.intValue
    val candles = market.candles.map { Candle(it.open, it.high, it.low, it.close, it.volume) }
    var low = Double.MAX_VALUE
    var high = -Double.MAX_VALUE
    var maxVolume = 1.0
    for (candle in candles) { low = min(low, candle.low); high = max(high, candle.high); maxVolume = max(maxVolume, candle.volume) }
    val averages = Averages.map { period -> candles.windowed(period) { window -> window.sumOf { it.close } / period } }
    val last = candles.last()
    val change = last.close - last.open
    val span = max(high - low, 0.0001)
    val lastAt = ((high - last.close) / span).toFloat() * 0.75f
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().height(28.dp).padding(horizontal = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            for (period in listOf("1m", "5m", "15m", "30m", "1h", "D", "W", "M", "Y")) Chip(period, period == "D", 0f, 12f)
            Spacer(Modifier.weight(1f))
            Label("MA · VOL", Modifier.padding(end = 8.dp), xs(MutedForeground))
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp).clipToBounds(),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            averages.forEachIndexed { ix, average ->
                Label("MA${Averages[ix]} ${price(average.last())}", style = xs(Series[ix]))
            }
            Label(
                "O ${price(last.open)} H ${price(last.high)} L ${price(last.low)} C ${price(last.close)}",
                style = xs(changeColor(change)),
            )
        }
        Box(Modifier.fillMaxWidth().weight(1f).padding(horizontal = 12.dp, vertical = 8.dp)) {
            Box(Modifier.fillMaxSize().drawBehind { drawChart(candles, averages, low, high, maxVolume) })
            Column(
                Modifier.width(PRICE_AXIS.dp).fillMaxHeight().align(Alignment.TopEnd)
                    .padding(start = 4.dp, bottom = TIME_AXIS.dp),
                verticalArrangement = Arrangement.SpaceBetween,
            ) {
                for (line in 0..4) Label(price(high - span * (line / 4.0) * 0.75), style = xs(MutedForeground))
            }
            BoxWithConstraints(Modifier.fillMaxSize()) {
                Box(
                    Modifier.align(Alignment.TopEnd).offset(y = maxHeight * lastAt)
                        .background(changeColor(change), RoundedCornerShape(RADIUS_SM.dp)).padding(horizontal = 4.dp),
                ) { Label(price(last.close), style = xs(Color.White)) }
            }
            Row(
                Modifier.fillMaxWidth().height(TIME_AXIS.dp).align(Alignment.BottomStart).padding(end = PRICE_AXIS.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                for (month in listOf("2026-04", "2026-05", "2026-06", "2026-07", "2026-08")) Label(month, style = xs(MutedForeground))
            }
        }
    }
}

private fun DrawScope.drawChart(candles: List<Candle>, averages: List<List<Double>>, low: Double, high: Double, maxVolume: Double) {
    val axis = PRICE_AXIS.dp.toPx()
    val timeAxis = TIME_AXIS.dp.toPx()
    val plotWidth = size.width - axis
    val plotHeight = size.height - timeAxis
    val priceHeight = plotHeight * 0.75f
    val volumeTop = plotHeight * 0.78f
    val volumeHeight = plotHeight * 0.22f
    val span = max(high - low, 0.0001)
    fun y(value: Double) = priceHeight * ((high - value) / span).toFloat()
    val step = plotWidth / candles.size
    fun x(ix: Int) = step * (ix + 0.5f)
    val unit = 1.dp.toPx()
    for (line in 0..4) drawRect(BorderColor, Offset(0f, priceHeight * line / 4f), Size(plotWidth, unit))
    for (line in 0..5) drawRect(BorderColor, Offset(plotWidth * line / 5f, 0f), Size(unit, plotHeight))
    val body = max(step * 0.7f, unit)
    candles.forEachIndexed { ix, candle ->
        val color = if (candle.close >= candle.open) Success else Danger
        val center = x(ix)
        drawRect(color, Offset(center - unit / 2, y(candle.high)), Size(unit, max(y(candle.low) - y(candle.high), unit)))
        val top = y(max(candle.open, candle.close))
        val bottom = y(min(candle.open, candle.close))
        drawRect(color, Offset(center - body / 2, top), Size(body, max(bottom - top, unit)))
        val height = volumeHeight * (candle.volume / maxVolume).toFloat()
        drawRect(color.copy(alpha = color.alpha * 0.5f), Offset(center - body / 2, volumeTop + volumeHeight - height), Size(body, height))
    }
    averages.forEachIndexed { series, average ->
        val period = Averages[series]
        val path = Path()
        average.forEachIndexed { ix, value ->
            if (ix == 0) path.moveTo(x(ix + period - 1), y(value)) else path.lineTo(x(ix + period - 1), y(value))
        }
        drawPath(path, Series[series], style = Stroke(1.2f * unit))
    }
    val closeY = y(candles.last().close)
    val lastX = x(candles.size - 1)
    val dash = PathEffect.dashPathEffect(floatArrayOf(3 * unit, 3 * unit))
    drawLine(MutedForeground, Offset(0f, closeY), Offset(plotWidth, closeY), unit, pathEffect = dash)
    drawLine(MutedForeground, Offset(lastX, 0f), Offset(lastX, plotHeight), unit, pathEffect = dash)
}

@Composable
private fun OrderBook(market: Market) {
    val (mid, updates) = market.book.value
    fun size(level: Int, bid: Boolean): Double {
        val salt = if (bid) 13 else 7
        return (((level * 37 + updates * salt + 11) % 900) + 20) * 100.0
    }
    val bids = (0 until BOOK_LEVELS).sumOf { size(it, true) }
    val asks = (0 until BOOK_LEVELS).sumOf { size(it, false) }
    val ratio = (bids / (bids + asks)).toFloat()
    val largest = (0 until BOOK_LEVELS).maxOf { max(size(it, true), size(it, false)) }.coerceAtLeast(1.0)
    Column(Modifier.fillMaxSize().padding(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Label("Bid ${fixed2(ratio * 100.0)}%", style = xs(Success))
            Box(
                Modifier.weight(1f).height(4.dp).drawBehind {
                    drawRoundRect(Danger, cornerRadius = CornerRadius(size.height / 2))
                    drawRoundRect(Success, size = Size(size.width * ratio, size.height), cornerRadius = CornerRadius(size.height / 2))
                },
            )
            Label("${fixed2((1 - ratio) * 100.0)}% Ask", style = xs(Danger))
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            for (bid in listOf(true, false)) {
                Column(Modifier.weight(1f)) {
                    val color = if (bid) Success else Danger
                    for (ix in 0 until BOOK_LEVELS) {
                        val offset = (ix + 1) * 0.01
                        val value = if (bid) mid - offset else mid + offset
                        val level = size(ix, bid)
                        val share = (level / largest).toFloat()
                        Row(
                            Modifier.fillMaxWidth().height(20.dp)
                                .drawBehind {
                                    val width = size.width * share
                                    drawRect(color.copy(alpha = 0.12f), Offset(if (bid) size.width - width else 0f, 0f), Size(width, size.height))
                                }
                                .padding(horizontal = 4.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Label("${ix + 1}", Modifier.width(16.dp), xs(MutedForeground))
                            Label(price(value), Modifier.weight(1f), xs(color))
                            Row(Modifier.width(48.dp), horizontalArrangement = Arrangement.End) { Label(amount(level), style = xs(Foreground)) }
                            Row(Modifier.width(24.dp), horizontalArrangement = Arrangement.End) {
                                Label("(${(ix * 7 + updates) % 30 + 1})", style = xs(MutedForeground))
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun TimeAndSales(market: Market) {
    market.tape.intValue
    val trades = market.trades.toList()
    Row(
        Modifier.fillMaxSize().padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        for (half in listOf(0 until TRADES / 2, TRADES / 2 until TRADES)) {
            Column(Modifier.weight(1f)) {
                // Keyed by the trade, so a new trade on top moves the rows
                // below instead of recomposing each with its neighbour's.
                for (ix in half) {
                    val trade = trades[ix]
                    key(trade.time) { TradeRow(trade) }
                }
            }
        }
    }
}

@Composable
private fun TradeRow(trade: Trade) {
    val color = if (trade.buy) Success else Danger
    Row(
        Modifier.fillMaxWidth().height(18.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Label(
            String.format(java.util.Locale.ROOT, "%02d:%02d:%02d", trade.time / 3_600, trade.time / 60 % 60, trade.time % 60),
            style = xs(MutedForeground),
        )
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.End) { Label(price(trade.price), style = xs(color)) }
        Row(Modifier.width(48.dp), horizontalArrangement = Arrangement.End) {
            Label(grouped(trade.size.toDouble(), 0), style = xs(Foreground))
        }
        Box(Modifier.size(2.dp, 12.dp).background(color, RoundedCornerShape(1.dp)))
    }
}

private val Buckets = listOf("Large buy", "Medium buy", "Small buy", "Small sell", "Medium sell", "Large sell")

@Composable
private fun TradeStats(market: Market) {
    val values = market.buckets.value
    val total = values.sum()
    val colors = listOf(
        Success, Success.copy(alpha = 0.7f), Success.copy(alpha = 0.4f),
        Danger.copy(alpha = 0.4f), Danger.copy(alpha = 0.7f), Danger,
    )
    val inflow = values.take(3).sum() - values.drop(3).sum()
    Row(
        Modifier.fillMaxSize().padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(112.dp).drawBehind {
                val stroke = 14.dp.toPx()
                val radius = min(size.width, size.height) / 2 - 8.dp.toPx()
                val topLeft = Offset(center.x - radius, center.y - radius)
                var start = -90f
                values.forEachIndexed { ix, value ->
                    val sweep = (value / total).toFloat() * 360f
                    drawArc(colors[ix], start, sweep, useCenter = false, topLeft = topLeft, size = Size(radius * 2, radius * 2), style = Stroke(stroke))
                    start += sweep
                }
            },
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            values.forEachIndexed { ix, value ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(8.dp).background(colors[ix], RoundedCornerShape(4.dp)))
                    Label(Buckets[ix], Modifier.weight(1f), xs(MutedForeground))
                    Row(Modifier.width(64.dp), horizontalArrangement = Arrangement.End) { Label(amount(value), style = xs(Foreground)) }
                    Row(Modifier.width(48.dp), horizontalArrangement = Arrangement.End) {
                        Label("${fixed2(value / total * 100)}%", style = xs(Foreground))
                    }
                }
            }
            Row(
                Modifier.fillMaxWidth().hairlines(top = true).padding(top = 4.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Label("Net inflow", style = xs(Foreground))
                Label(amount(abs(inflow)), style = xs(changeColor(inflow)))
            }
        }
    }
}

@Composable
private fun StatusBar(tick: Int) {
    Row(
        Modifier.fillMaxWidth().height(24.dp).hairlines(top = true).padding(horizontal = 12.dp).clipToBounds(),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        listOf("HSI", "HSCEI", "HSTECH", "SSE", "SPX", "IXIC").forEachIndexed { ix, name ->
            val value = 20_000.0 + (ix * 1_234 + tick * 7) * 0.37
            val change = (if (ix % 2 == 0) -1.0 else 1.0) * (ix + 1) * 12.3
            val color = changeColor(change)
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Label(name, style = xs(MutedForeground))
                Label(price(value), style = xs(color))
                Label(signed(change), style = xs(color))
                Label(percent(change / value * 100), style = xs(color))
            }
        }
        Spacer(Modifier.weight(1f))
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(8.dp).background(Success, RoundedCornerShape(4.dp)))
            Label("Connected · Level 2", style = xs(Foreground))
        }
        Label(String.format(java.util.Locale.ROOT, "10:%02d:%02d EST", tick / 60 % 60, tick % 60), style = xs(MutedForeground))
    }
}

package dev.perfcompare.compose

import dev.perfcompare.shared.*
import android.graphics.Bitmap
import android.util.Log
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.IntState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.isActive

// The gauntlet: one screen that loads every stage of a frame at once, heavy
// enough at its device's tier that no framework holds 60 fps. `gauntlet.rs`
// draws the same screen, element for element, in Cranpose's own idiom; the
// benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time, so both frameworks do the same work per frame.

private val Ink = Color(0xFF111827)
private val Body = Color(0xFF374151)
private val Muted = Color(0xFF6B7280)
private val Hairline = Color(0xFFE5E7EB)
private val Panel = Color(0xFFE2E8F0)
private val Up = Color(0xFF16A34A)
private val Down = Color(0xFFDC2626)
private val LevelBackground = arrayOf(Color(0xFFF1F5F9), Color(0xFFCBD5E1))

/** Blocks of five card rows and a cluster: no measurement window reaches the end. */
private const val BLOCKS = 200_000

/** How far the grid scrolls each frame, in dp. */
private const val SCROLL_PER_FRAME = 3f

/** What `am start` asked of the gauntlet. */
@Immutable
data class GauntletLoad(
    /** Load tier, 1 to 8. */
    val tier: Int,
    /** Stop on this frame and hold still, for picture comparisons; 0 runs on. */
    val freeze: Int,
)

@Composable
fun ColumnScope.GauntletScreen(load: GauntletLoad) {
    val tier = remember(load.tier) { gauntletTier(load.tier) }
    val frame = remember { mutableIntStateOf(0) }
    val state = rememberLazyGridState()
    val posts = remember { posts() }
    val avatars = remember {
        List(AVATAR_COUNT) {
            Bitmap.createBitmap(avatarArgb(it), AVATAR_SIZE, AVATAR_SIZE, Bitmap.Config.ARGB_8888)
                .asImageBitmap()
        }
    }
    val tickers = remember(tier) { tickers(tier.tickers) }
    val scrollPx = with(LocalDensity.current) { SCROLL_PER_FRAME.dp.toPx() }

    LaunchedEffect(load.freeze) {
        var index = 0
        while (isActive) {
            withFrameNanos { }
            index++
            frame.intValue = index
            state.dispatchRawDelta(scrollPx)
            if (load.freeze in 1..index) {
                Log.i(TAG, "PERF frozen frame=$index")
                break
            }
        }
    }

    val s = tier.scale
    Column(
        Modifier
            .fillMaxWidth()
            .weight(1f)
            // The width is read while measuring, not composing: the frame
            // measures everything again without recomposing anything.
            .layout { measurable, constraints ->
                val width = (constraints.maxWidth * widthFraction(frame.intValue)).roundToInt()
                val placeable = measurable.measure(constraints.copy(minWidth = width, maxWidth = width))
                layout(constraints.maxWidth, placeable.height) { placeable.place(0, 0) }
            },
    ) {
        TickerPanel(tickers, frame, s)
        val perBlock = CARD_ROWS_PER_CLUSTER * tier.columns + 1
        LazyVerticalGrid(
            columns = GridCells.Fixed(tier.columns),
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f),
            state = state,
            contentPadding = PaddingValues((8 * s).dp),
            verticalArrangement = Arrangement.spacedBy((8 * s).dp),
            horizontalArrangement = Arrangement.spacedBy((8 * s).dp),
        ) {
            items(
                count = BLOCKS * perBlock,
                key = { it },
                span = { if (it % perBlock == perBlock - 1) GridItemSpan(maxLineSpan) else GridItemSpan(1) },
                contentType = { if (it % perBlock == perBlock - 1) 1 else 0 },
            ) { item ->
                val block = item / perBlock
                val within = item % perBlock
                if (within == perBlock - 1) {
                    Level(block, tier.depth, s)
                } else {
                    val card = block * (perBlock - 1) + within
                    Card(posts[card % posts.size], avatars[card % AVATAR_COUNT], card, frame, s)
                }
            }
        }
    }
}

@Composable
private fun TickerPanel(tickers: List<Ticker>, frame: IntState, s: Float) {
    FlowRow(
        Modifier
            .fillMaxWidth()
            .background(Panel)
            .padding((6 * s).dp),
        horizontalArrangement = Arrangement.spacedBy((4 * s).dp),
        verticalArrangement = Arrangement.spacedBy((4 * s).dp),
    ) {
        tickers.forEachIndexed { index, ticker -> TickerTile(ticker, index, frame, s) }
    }
}

@Composable
private fun TickerTile(ticker: Ticker, index: Int, frame: IntState, s: Float) {
    Row(
        Modifier
            .background(Color.White, RoundedCornerShape((6 * s).dp))
            .padding(horizontal = (6 * s).dp, vertical = (3 * s).dp),
        horizontalArrangement = Arrangement.spacedBy((4 * s).dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(ticker.symbol, style = textStyle(10f * s, Ink, true))
        TickerPrice(ticker, frame, s)
        TickerChange(ticker, frame, s)
        val color = PALETTE[index % PALETTE.size]
        Box(
            Modifier
                .size((20 * s).dp, (4 * s).dp)
                .background(Hairline, RoundedCornerShape((2 * s).dp))
                .drawBehind {
                    val cents = tickerCents(ticker, frame.intValue)
                    val share = ((cents - ticker.baseCents + ticker.swingCents).toFloat() /
                        (2 * ticker.swingCents)).coerceIn(0f, 1f)
                    drawRoundRect(
                        color,
                        size = Size(size.width * share, size.height),
                        cornerRadius = CornerRadius((2 * s).dp.toPx()),
                    )
                },
        )
    }
}

/** The price on this frame: its own scope, so the frame recomposes only it. */
@Composable
private fun TickerPrice(ticker: Ticker, frame: IntState, s: Float) {
    BasicText(centsText(tickerCents(ticker, frame.intValue)), style = textStyle(10f * s, Body, false))
}

/** The signed change, green or red, in its own scope like the price. */
@Composable
private fun TickerChange(ticker: Ticker, frame: IntState, s: Float) {
    val cents = tickerCents(ticker, frame.intValue)
    BasicText(
        changeText(ticker, cents),
        style = textStyle(10f * s, if (cents >= ticker.baseCents) Up else Down, false),
    )
}

@Composable
private fun Card(post: Post, avatar: ImageBitmap, card: Int, frame: IntState, s: Float) {
    val shape = RoundedCornerShape((12 * s).dp)
    Box {
        Column(
            Modifier
                .fillMaxWidth()
                .shadow((3 * s).dp, shape)
                .background(Color.White, shape)
                .border(1.dp, Hairline, shape)
                .padding((10 * s).dp),
            verticalArrangement = Arrangement.spacedBy((6 * s).dp),
        ) {
            CardHeader(post, avatar, s)
            BasicText(
                post.body,
                style = textStyle(12f * s, Body, false),
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
            )
            Progress(card, frame, s)
            Sparkline(card, post.color, frame, s)
            Chips(post, s)
            Footer(post, s)
        }
        if (card % 5 == 0) {
            // A translucent tag tilting with the frame in its graphics layer:
            // drawn again each frame, never measured again.
            BasicText(
                "HOT",
                Modifier
                    .align(Alignment.TopEnd)
                    .padding((6 * s).dp)
                    .graphicsLayer {
                        rotationZ = badgeDegrees(card, frame.intValue)
                        alpha = 0.9f
                    }
                    .background(PALETTE[0], RoundedCornerShape((8 * s).dp))
                    .padding(horizontal = (6 * s).dp, vertical = (2 * s).dp),
                style = textStyle(9f * s, Color.White, true),
            )
        }
    }
}

@Composable
private fun CardHeader(post: Post, avatar: ImageBitmap, s: Float) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy((8 * s).dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(
            avatar,
            contentDescription = null,
            modifier = Modifier
                .size((32 * s).dp)
                .clip(CircleShape),
            contentScale = ContentScale.Crop,
        )
        Column(Modifier.weight(1f)) {
            BasicText(
                post.title,
                style = textStyle(13f * s, Ink, true),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            BasicText(
                remember(post) { subtitle(post) },
                style = textStyle(11f * s, Muted, false),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** `by Ada Lovelace · #rust`: the author bold, the tag in its color. */
private fun subtitle(post: Post): AnnotatedString {
    val (tag, color) = post.tags[0]
    return buildAnnotatedString {
        append("by ")
        withStyle(SpanStyle(fontWeight = FontWeight.Bold)) { append(post.author) }
        append(" · ")
        withStyle(SpanStyle(color = GRADIENT_END[color])) { append(tag) }
    }
}

@Composable
private fun Progress(card: Int, frame: IntState, s: Float) {
    val color = PALETTE[card % PALETTE.size]
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy((6 * s).dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .weight(1f)
                .height((6 * s).dp)
                .background(Hairline, RoundedCornerShape((3 * s).dp))
                .drawBehind {
                    val share = progressPermille(card, frame.intValue) / 1000f
                    drawRoundRect(
                        color,
                        size = Size(size.width * share, size.height),
                        cornerRadius = CornerRadius((3 * s).dp.toPx()),
                    )
                },
        )
        ProgressLabel(card, frame, s)
    }
}

/** The percent beside the bar, in its own scope like the ticker's labels. */
@Composable
private fun ProgressLabel(card: Int, frame: IntState, s: Float) {
    BasicText("${progressPermille(card, frame.intValue) / 10}%", style = textStyle(10f * s, Muted, false))
}

@Composable
private fun Sparkline(card: Int, color: Int, frame: IntState, s: Float) {
    val line = PALETTE[color]
    Spacer(
        Modifier
            .fillMaxWidth()
            .height((36 * s).dp)
            .drawWithCache {
                val area = Path()
                val stroke = Path()
                val fill = Brush.verticalGradient(
                    listOf(line.copy(alpha = 0.25f), line.copy(alpha = 0f)),
                    startY = 0f,
                    endY = size.height,
                )
                val strokeStyle = Stroke(width = (1.5f * s).dp.toPx())
                val step = size.width / (GAUNTLET_SPARK_POINTS - 1)
                onDrawBehind {
                    val k = frame.intValue
                    area.rewind()
                    stroke.rewind()
                    area.moveTo(0f, size.height)
                    for (index in 0 until GAUNTLET_SPARK_POINTS) {
                        val x = index * step
                        val y = size.height * (1f - sparkValue(card, index, k))
                        area.lineTo(x, y)
                        if (index == 0) stroke.moveTo(x, y) else stroke.lineTo(x, y)
                    }
                    area.lineTo(size.width, size.height)
                    area.close()
                    drawPath(area, fill)
                    drawPath(stroke, line, style = strokeStyle)
                }
            },
    )
}

@Composable
private fun Chips(post: Post, s: Float) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy((4 * s).dp),
        verticalArrangement = Arrangement.spacedBy((4 * s).dp),
    ) {
        for ((tag, color) in post.tags) {
            BasicText(
                tag,
                Modifier
                    .background(CHIP_BACKGROUND[color], RoundedCornerShape((10 * s).dp))
                    .padding(horizontal = (8 * s).dp, vertical = (3 * s).dp),
                style = textStyle(10f * s, GRADIENT_END[color], false),
            )
        }
    }
}

private val FOOTER_LABELS = listOf("likes", "replies", "shares")

/**
 * Three counters split by dividers as tall as the tallest counter: an
 * intrinsic measurement on every pass.
 */
@Composable
private fun Footer(post: Post, s: Float) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min),
    ) {
        FOOTER_LABELS.forEachIndexed { index, label ->
            if (index > 0) {
                Box(
                    Modifier
                        .width(1.dp)
                        .fillMaxHeight()
                        .background(Hairline),
                )
            }
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                BasicText(post.stats[index], style = textStyle(12f * s, Ink, true))
                BasicText(label, style = textStyle(9f * s, Muted, false))
            }
        }
    }
}

/**
 * `depth` levels nested inside one another, each a row of three labels above
 * the next level.
 */
@Composable
private fun Level(cluster: Int, remaining: Int, s: Float) {
    Column(
        Modifier
            .fillMaxWidth()
            .background(LevelBackground[remaining % 2], RoundedCornerShape((4 * s).dp))
            .padding(start = (3 * s).dp, top = (1 * s).dp, end = (1 * s).dp, bottom = (1 * s).dp),
        verticalArrangement = Arrangement.spacedBy((1 * s).dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy((2 * s).dp)) {
            for (chip in 0 until 3) {
                BasicText(
                    "C$cluster.L$remaining.$chip",
                    Modifier
                        .weight(1f)
                        .background(PALETTE[(remaining + chip + cluster) % PALETTE.size], RoundedCornerShape((2 * s).dp)),
                    style = textStyle(9f * s, Color.White, false),
                )
            }
        }
        if (remaining > 0) Level(cluster, remaining - 1, s)
    }
}

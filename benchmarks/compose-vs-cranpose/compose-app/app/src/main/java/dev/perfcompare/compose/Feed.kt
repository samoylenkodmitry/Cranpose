package dev.perfcompare.compose

import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.isActive

private val Ink = Color(0xFF111827)
private val Muted = Color(0xFF4B5563)

/** List rows: the posts repeat, so no scroll speed reaches the end of the list inside a window. */
private const val ROWS = 1_000_000

/** Everything a card draws, in dp and sp before [scale] shrinks it. */
@Immutable
data class FeedLoad(
    /** Auto-scroll speed in dp per second; 0 holds still for layout parity checks. */
    val speed: Float,
    /** Cards side by side in each list row. */
    val columns: Int,
    /** Comment rows under each card. */
    val comments: Int,
    /** Multiplies every size and text size: below 1 fits more cards on screen. */
    val scale: Float,
)

@Composable
fun ColumnScope.FeedScreen(load: FeedLoad) {
    val posts = remember { posts() }
    val state = rememberLazyListState()
    val density = LocalDensity.current.density
    LaunchedEffect(load.speed) {
        if (load.speed == 0f) return@LaunchedEffect
        val start = withFrameNanos { it }
        var last = start
        var nextReport = 5f
        while (isActive) {
            val now = withFrameNanos { it }
            val dt = (now - last) / 1e9f
            last = now
            state.scrollBy(load.speed * density * dt)
            val elapsed = (now - start) / 1e9f
            if (elapsed >= nextReport) {
                nextReport += 5f
                Log.i(TAG, "PERF feed t=$elapsed first_visible=${state.firstVisibleItemIndex}")
            }
        }
    }
    val s = load.scale
    LazyColumn(
        Modifier
            .fillMaxWidth()
            .weight(1f),
        state = state,
        contentPadding = PaddingValues(vertical = (12 * s).dp),
        verticalArrangement = Arrangement.spacedBy((12 * s).dp),
    ) {
        items(ROWS, key = { it }, contentType = { 0 }) { row -> PostRow(posts, row, load) }
    }
}

@Composable
private fun PostRow(posts: List<Post>, row: Int, load: FeedLoad) {
    val s = load.scale
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = (12 * s).dp),
        horizontalArrangement = Arrangement.spacedBy((12 * s).dp),
    ) {
        for (column in 0 until load.columns) {
            PostCard(posts[(row * load.columns + column) % posts.size], load.comments, s)
        }
    }
}

@Composable
private fun RowScope.PostCard(post: Post, comments: Int, s: Float) {
    val thread = remember(post) { comments(post.id, comments) }
    Column(
        Modifier
            .weight(1f)
            .background(Color.White, RoundedCornerShape((16 * s).dp))
            .padding((16 * s).dp),
        verticalArrangement = Arrangement.spacedBy((10 * s).dp),
    ) {
        PostHeader(post, s)
        BasicText(post.title, style = textStyle(18 * s, Ink, true), maxLines = 2, overflow = TextOverflow.Ellipsis)
        BasicText(
            post.body,
            style = textStyle(15 * s, Color(0xFF374151), false),
            maxLines = 4,
            overflow = TextOverflow.Ellipsis,
        )
        MediaChart(post, s)
        // Chips and counters wrap onto new lines on narrow cards.
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy((8 * s).dp),
            verticalArrangement = Arrangement.spacedBy((6 * s).dp),
        ) {
            for ((tag, color) in post.tags) {
                BasicText(
                    tag,
                    Modifier
                        .background(CHIP_BACKGROUND[color], RoundedCornerShape((14 * s).dp))
                        .padding(horizontal = (12 * s).dp, vertical = (6 * s).dp),
                    style = textStyle(13 * s, GRADIENT_END[color], false),
                )
            }
        }
        PostActions(post, s)
        for (comment in thread) CommentRow(comment, s)
    }
}

@Composable
private fun CommentRow(comment: Comment, s: Float) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy((8 * s).dp)) {
        Box(
            Modifier
                .size((24 * s).dp)
                .background(PALETTE[comment.color], RoundedCornerShape((12 * s).dp)),
        )
        Column(Modifier.weight(1f)) {
            BasicText(comment.author, style = textStyle(13 * s, Ink, true))
            BasicText(comment.text, style = textStyle(13 * s, Muted, false))
        }
    }
}

@Composable
private fun PostHeader(post: Post, s: Float) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy((12 * s).dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size((44 * s).dp)
                .background(PALETTE[post.color], RoundedCornerShape((22 * s).dp)),
            contentAlignment = Alignment.Center,
        ) {
            BasicText(post.initials, style = textStyle(16 * s, Color.White, true))
        }
        Column(Modifier.weight(1f)) {
            BasicText(post.author, style = textStyle(16 * s, Ink, true), maxLines = 1, overflow = TextOverflow.Ellipsis)
            BasicText(
                post.handle,
                style = textStyle(13 * s, Color(0xFF6B7280), false),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Box(
            Modifier
                .size((28 * s).dp)
                .background(Color(0xFFE0E7FF), RoundedCornerShape((14 * s).dp)),
        )
    }
}

@Composable
private fun MediaChart(post: Post, s: Float) {
    Box(
        Modifier
            .fillMaxWidth()
            .height((140 * s).dp)
            .drawWithCache {
                val gradient = Brush.verticalGradient(
                    listOf(PALETTE[post.color], GRADIENT_END[post.color]),
                    0f,
                    size.height,
                )
                val barColor = Color.White.copy(alpha = 0.8f)
                val bubbleColor = Color.White.copy(alpha = 0.25f)
                val pad = (12 * s).dp.toPx()
                val slot = (size.width - pad * 2f) / BAR_COUNT
                val barWidth = slot * 0.6f
                val chartHeight = size.height - pad * 2f
                val mediaCorner = CornerRadius((12 * s).dp.toPx())
                val barCorner = CornerRadius((3 * s).dp.toPx())
                val bubbleRadius = (22 * s).dp.toPx()
                onDrawBehind {
                    drawRoundRect(gradient, cornerRadius = mediaCorner)
                    drawCircle(bubbleColor, bubbleRadius, Offset(size.width * 0.8f, size.height * 0.25f))
                    for (index in 0 until BAR_COUNT) {
                        val height = post.bars[index] * chartHeight
                        drawRoundRect(
                            barColor,
                            topLeft = Offset(pad + index * slot + (slot - barWidth) / 2f, size.height - pad - height),
                            size = Size(barWidth, height),
                            cornerRadius = barCorner,
                        )
                    }
                }
            },
    )
}

@Composable
private fun PostActions(post: Post, s: Float) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy((20 * s).dp),
        verticalArrangement = Arrangement.spacedBy((6 * s).dp),
    ) {
        for (stat in post.stats) {
            Row(
                horizontalArrangement = Arrangement.spacedBy((6 * s).dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier
                        .size((18 * s).dp)
                        .background(Color(0xFF9CA3AF), RoundedCornerShape((9 * s).dp)),
                )
                BasicText(stat, style = textStyle(13 * s, Muted, false))
            }
        }
    }
}

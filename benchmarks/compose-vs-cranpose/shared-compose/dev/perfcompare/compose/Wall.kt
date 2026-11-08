package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.perfcompare.shared.WALL_BACKGROUND_ARGB
import dev.perfcompare.shared.WALL_CLOCK_HEIGHT
import dev.perfcompare.shared.WALL_CLOCK_TEXT
import dev.perfcompare.shared.WALL_CORNER
import dev.perfcompare.shared.WALL_CURSOR_WIDTH
import dev.perfcompare.shared.WALL_GAP
import dev.perfcompare.shared.WALL_INK_ARGB
import dev.perfcompare.shared.wallClock
import dev.perfcompare.shared.wallCursor
import dev.perfcompare.shared.wallDotArgb
import dev.perfcompare.shared.wallDotSize
import dev.perfcompare.shared.wallFillArgb
import dev.perfcompare.shared.wallGrid
import dev.perfcompare.shared.wallLabel
import dev.perfcompare.shared.wallLabelSize
import dev.perfcompare.shared.wallPadding
import dev.perfcompare.shared.wallTier
import dev.perfcompare.shared.wallValue
import dev.perfcompare.shared.wallValueSize
import kotlinx.coroutines.isActive

// The still wall in Compose (`../../../../README.md`): a window of tiles that
// never change, under a clock line whose text changes every frame, with a
// cursor outline that rests on another tile every frame.
//
// In Compose's idiom the wall is one call whose arguments never change, so no
// tile composes again; the clock is the one composable that reads the frame
// while composing; the cursor moves in a graphics layer whose block reads the
// frame, so its frame records nothing again.

val WALL_BACKGROUND = Color(WALL_BACKGROUND_ARGB)
private val INK = Color(WALL_INK_ARGB)

/** The wall's tier and the frame it freezes on; 0 runs on. */
@Immutable
data class WallLoad(val tier: Int, val freeze: Int)

@Composable
fun ColumnScope.WallScreen(load: WallLoad) {
    val tier = remember(load.tier) { wallTier(load.tier) }
    val frame = remember { mutableIntStateOf(0) }
    LaunchedEffect(load.freeze) {
        var index = 0
        while (isActive) {
            withFrameNanos { }
            index++
            frame.intValue = index
            if (load.freeze in 1..index) {
                perfLog("PERF frozen frame=$index")
                break
            }
        }
    }
    Clock(frame)
    BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
        val (columns, rows) = wallGrid(tier, maxWidth.value, maxHeight.value)
        Wall(columns, rows, tier.tileWidth, tier.tileHeight)
        Cursor(frame, columns, columns * rows, tier.tileWidth, tier.tileHeight)
    }
}

// The line above the wall: the frame's number, new each frame.
@Composable
private fun Clock(frame: IntState) {
    Box(
        Modifier.fillMaxWidth().height(WALL_CLOCK_HEIGHT.dp).padding(horizontal = 8.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        BasicText(wallClock(frame.intValue), style = textStyle(WALL_CLOCK_TEXT, INK, false))
    }
}

// The tiles, [columns] by [rows] of them [width] by [height] points each,
// `WALL_GAP` apart.
@Composable
private fun Wall(columns: Int, rows: Int, width: Float, height: Float) {
    Column(
        Modifier.padding(WALL_GAP.dp),
        verticalArrangement = Arrangement.spacedBy(WALL_GAP.dp),
    ) {
        for (row in 0 until rows) {
            Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP.dp)) {
                for (column in 0 until columns) {
                    Tile(row * columns + column, width, height)
                }
            }
        }
    }
}

// One tile: its label over a dot and its value, on its fill.
@Composable
private fun Tile(index: Int, width: Float, height: Float) {
    val dot = wallDotSize(height)
    Column(
        Modifier
            .size(width.dp, height.dp)
            .background(Color(wallFillArgb(index)), RoundedCornerShape(WALL_CORNER.dp))
            .padding(wallPadding(height).dp),
    ) {
        BasicText(
            wallLabel(index),
            style = textStyle(wallLabelSize(height), INK, true),
            overflow = TextOverflow.Clip,
            softWrap = false,
            maxLines = 1,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy((dot * 0.5f).dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(dot.dp).background(Color(wallDotArgb(index)), RoundedCornerShape((dot * 0.5f).dp)))
            BasicText(
                wallValue(index),
                style = textStyle(wallValueSize(height), INK, false),
                overflow = TextOverflow.Clip,
                softWrap = false,
                maxLines = 1,
            )
        }
    }
}

// The cursor's outline, over the tile `wallCursor` names for the frame: its
// layer block reads the frame, so the frame moves it with no composition,
// layout or drawing of its own.
@Composable
private fun Cursor(frame: IntState, columns: Int, tiles: Int, width: Float, height: Float) {
    Box(
        Modifier
            .size(width.dp, height.dp)
            .graphicsLayer {
                val tile = wallCursor(frame.intValue, tiles)
                translationX = (WALL_GAP + (tile % columns) * (width + WALL_GAP)).dp.toPx()
                translationY = (WALL_GAP + (tile / columns) * (height + WALL_GAP)).dp.toPx()
            }
            .border(WALL_CURSOR_WIDTH.dp, INK, RoundedCornerShape(WALL_CORNER.dp)),
    )
}

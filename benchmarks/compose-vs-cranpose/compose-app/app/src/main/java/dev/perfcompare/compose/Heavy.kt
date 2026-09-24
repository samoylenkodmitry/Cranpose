package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import kotlin.math.sin

// Layout-bound screens: the container width follows the frame clock, so every
// frame measures and places the whole tree again while nothing below the
// screen recomposes. The `layered` variants give every node a static rotated
// graphics layer.

private val LevelBackground = arrayOf(Color(0xFFE2E8F0), Color(0xFFCBD5E1))

private fun widthFraction(seconds: Float) = 0.7f + 0.3f * (0.5f + 0.5f * sin(seconds * 2f))

private fun Modifier.tilt(layered: Boolean, degrees: Float) =
    if (layered) graphicsLayer(rotationZ = degrees) else this

/** A flat grid of `rows × columns` cells whose labels wrap as the width moves. */
@Composable
fun ColumnScope.GridScreen(layered: Boolean, rows: Int, columns: Int) {
    val seconds = rememberFrameSeconds()
    Column(
        Modifier
            .fillMaxWidth(widthFraction(seconds.value))
            .weight(1f),
    ) {
        for (row in 0 until rows) GridRow(row, columns, layered)
    }
}

@Composable
private fun ColumnScope.GridRow(row: Int, columns: Int, layered: Boolean) {
    Row(
        Modifier
            .fillMaxWidth()
            .weight(1f),
    ) {
        for (column in 0 until columns) GridCell(row * columns + column, layered)
    }
}

@Composable
private fun RowScope.GridCell(index: Int, layered: Boolean) {
    Box(
        Modifier
            .weight(1f)
            .fillMaxHeight()
            .padding(1.dp)
            .tilt(layered, ((index % 7) - 3) * 2f)
            .background(PALETTE[index % PALETTE.size], RoundedCornerShape(3.dp)),
        contentAlignment = Alignment.Center,
    ) {
        BasicText("cell $index", style = textStyle(9f, Color.White, false))
    }
}

/**
 * `depth` nested levels, each a row of `chips` labels above the next level: a
 * thread of replies nested inside one another.
 */
@Composable
fun ColumnScope.DeepScreen(layered: Boolean, depth: Int, chips: Int) {
    val seconds = rememberFrameSeconds()
    Box(
        Modifier
            .fillMaxWidth(widthFraction(seconds.value))
            .weight(1f),
    ) {
        Level(depth, chips, layered)
    }
}

@Composable
private fun Level(remaining: Int, chips: Int, layered: Boolean) {
    Column(
        Modifier
            .fillMaxWidth()
            .tilt(layered, if (remaining % 2 == 0) 0.6f else -0.6f)
            .background(LevelBackground[remaining % 2], RoundedCornerShape(4.dp))
            .padding(start = 3.dp, top = 1.dp, end = 1.dp, bottom = 1.dp),
        verticalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            for (chip in 0 until chips) {
                BasicText(
                    "L$remaining.$chip",
                    Modifier
                        .weight(1f)
                        .background(PALETTE[(remaining + chip) % PALETTE.size], RoundedCornerShape(2.dp)),
                    style = textStyle(8f, Color.White, false),
                )
            }
        }
        if (remaining > 0) Level(remaining - 1, chips, layered)
    }
}

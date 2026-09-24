package dev.perfcompare.compose

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.isActive
import java.io.File
import kotlin.math.sin

private const val MEDIA_HEIGHT = 140f

// The device's Roboto faces, the same two files the Cranpose app loads.
private val Roboto = FontFamily(
    Font(File("/system/fonts/Roboto-Regular.ttf"), FontWeight.Normal),
    Font(File("/system/fonts/Roboto-Bold.ttf"), FontWeight.Bold),
)
private val Ink = Color(0xFF111827)
private val Up = Color(0xFF16A34A)
private val Down = Color(0xFFDC2626)

// Line height is explicit because the two frameworks read different vertical
// metrics from the same font; 1.4 em makes both lay out identical lines.
fun textStyle(sizeSp: Float, color: Color, bold: Boolean) = TextStyle(
    color = color,
    fontSize = sizeSp.sp,
    fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal,
    fontFamily = Roboto,
    lineHeight = 1.4f.em,
)

@Composable
fun TopBar(title: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(56.dp)
            .background(Color(0xFF1E2A4A))
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(title, style = textStyle(20f, Color.White, true))
    }
}

/** Seconds since this screen's first frame, written once per frame. */
@Composable
fun rememberFrameSeconds(): State<Float> {
    val seconds = remember { mutableFloatStateOf(0f) }
    LaunchedEffect(Unit) {
        val start = withFrameNanos { it }
        while (isActive) {
            val now = withFrameNanos { it }
            seconds.floatValue = (now - start) / 1e9f
        }
    }
    return seconds
}

// -------------------------------------------------------------- ticker

/** [rows] quotes share the screen height; past 24 rows the type shrinks with them. */
@Composable
fun ColumnScope.TickerScreen(rows: Int) {
    val quotes = remember { quotes(rows) }
    val seconds = rememberFrameSeconds()
    val scale = minOf(1f, 24f / rows)
    Column(
        Modifier
            .fillMaxWidth()
            .weight(1f)
            .padding(vertical = 8.dp),
    ) {
        for (quote in quotes) QuoteRow(quote, seconds, scale)
    }
}

@Composable
private fun ColumnScope.QuoteRow(quote: Quote, seconds: State<Float>, scale: Float) {
    // The only read of the frame clock: each row recomposes on its own.
    val wave = sin(seconds.value * quote.speed + quote.phase)
    val price = quote.base * (1f + 0.04f * wave)
    val change = 4f * wave
    val color = if (change >= 0f) Up else Down
    Row(
        Modifier
            .fillMaxWidth()
            .weight(1f)
            .padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(quote.symbol, Modifier.width(64.dp), style = textStyle(14f * scale, Ink, true))
        BasicText(fixed2(price, false), Modifier.width(90.dp), style = textStyle(14f * scale, Ink, false))
        BasicText(
            fixed2(change, true) + "%",
            Modifier.width(72.dp),
            style = textStyle(13f * scale, color, false),
        )
        Box(
            Modifier
                .size(width = (8f + 72f * (0.5f + 0.5f * wave)).dp, height = (10f * scale).dp)
                .background(color, RoundedCornerShape(5.dp)),
        )
    }
}

// ----------------------------------------------------------- particles

@Composable
fun ColumnScope.ParticlesScreen(count: Int) {
    val particles = remember { particles(count) }
    val colors = remember { Array(PALETTE.size) { PALETTE[it].copy(alpha = 0.85f) } }
    val seconds = rememberFrameSeconds()
    Canvas(
        Modifier
            .fillMaxWidth()
            .weight(1f)
            .background(Color(0xFF0B1020)),
    ) {
        // Read in the draw phase only: frames redraw, nothing recomposes.
        val t = seconds.value
        val width = size.width
        val height = size.height
        val scale = density
        val corner = CornerRadius(3f * scale)
        for (index in 0 until count) {
            val x = wrapUnit(particles.x[index] + particles.vx[index] * t) * width
            val y = wrapUnit(particles.y[index] + particles.vy[index] * t) * height
            val color = colors[particles.color[index]]
            val extent = particles.size[index] * scale
            if (isCircle(index, count)) {
                drawCircle(color, extent, Offset(x, y))
            } else {
                drawRoundRect(color, Offset(x, y), Size(extent, extent), corner)
            }
        }
    }
}

// -------------------------------------------------------------- layers

/**
 * [columns] × [rows] tiles fill the screen; past 6 columns the gaps and type
 * shrink with the tiles. [opaque] keeps every tile at full alpha
 * (`--ez opaque true`), an ablation that separates the cost of translucent
 * layers from transformed ones.
 */
@Composable
fun ColumnScope.LayersScreen(opaque: Boolean, columns: Int, rows: Int) {
    val seconds = rememberFrameSeconds()
    val scale = minOf(1f, 6f / columns)
    Column(
        Modifier
            .fillMaxWidth()
            .weight(1f)
            .padding(4.dp),
        verticalArrangement = Arrangement.spacedBy((8f * scale).dp),
    ) {
        for (row in 0 until rows) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .weight(1f),
                horizontalArrangement = Arrangement.spacedBy((8f * scale).dp),
            ) {
                for (column in 0 until columns) Tile(row * columns + column, seconds, opaque, scale)
            }
        }
    }
}

@Composable
private fun RowScope.Tile(index: Int, seconds: State<Float>, opaque: Boolean, scale: Float) {
    val phase = index.toFloat()
    Box(
        Modifier
            .weight(1f)
            .fillMaxHeight()
            // Read in the layer block only: frames update layer properties,
            // nothing recomposes or redraws.
            .graphicsLayer {
                val t = seconds.value
                rotationZ = (t * 90f + phase * 13f) % 360f
                val scale = 0.85f + 0.15f * sin(t * 3f + phase * 0.4f)
                scaleX = scale
                scaleY = scale
                if (!opaque) alpha = 0.65f + 0.35f * (0.5f + 0.5f * sin(t * 2f + phase * 0.7f))
            }
            .background(PALETTE[index % PALETTE.size], RoundedCornerShape((12f * scale).dp)),
        contentAlignment = Alignment.Center,
    ) {
        BasicText(index.toString(), style = textStyle(16f * scale, Color.White, true))
    }
}

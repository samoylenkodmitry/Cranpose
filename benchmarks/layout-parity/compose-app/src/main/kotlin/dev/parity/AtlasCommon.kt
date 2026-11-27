package dev.parity

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

internal val BG_CANVAS = Color(0xFFF1F5F9)
internal val BG_CARD = Color(0xFFFFFFFF)
internal val BG_SUBTLE = Color(0xFFF8FAFC)
internal val BG_TRACK = Color(0xFFE2E8F0)
internal val BORDER_CARD = Color(0xFFCBD5E1)
internal val BORDER_STRONG = Color(0xFF94A3B8)
internal val INK = Color(0xFF0F172A)
internal val MUTED = Color(0xFF475569)
internal val HEADER_BG = Color(0xFF0F172A)
internal val FOOTER_BG = Color(0xFF1E293B)

internal val BLUE = Color(0xFF2563EB)
internal val BLUE_SOFT = Color(0xFFDBEAFE)
internal val INDIGO = Color(0xFF4F46E5)
internal val INDIGO_SOFT = Color(0xFFE0E7FF)
internal val VIOLET = Color(0xFF7C3AED)
internal val VIOLET_SOFT = Color(0xFFEDE9FE)
internal val ROSE = Color(0xFFE11D48)
internal val ROSE_SOFT = Color(0xFFFFE4E6)
internal val RED = Color(0xFFDC2626)
internal val RED_SOFT = Color(0xFFFEE2E2)
internal val AMBER = Color(0xFFD97706)
internal val AMBER_SOFT = Color(0xFFFEF3C7)
internal val EMERALD = Color(0xFF059669)
internal val EMERALD_SOFT = Color(0xFFD1FAE5)
internal val TEAL = Color(0xFF0D9488)
internal val TEAL_SOFT = Color(0xFFCCFBF1)
internal val CYAN = Color(0xFF0284C7)
internal val CYAN_SOFT = Color(0xFFE0F2FE)
internal val SLATE = Color(0xFF64748B)

internal val ACCENTS = listOf(
    BLUE, EMERALD, AMBER, ROSE, VIOLET, CYAN, TEAL, INDIGO, RED,
)

data class RecordedRect(
    val x: Float,
    val y: Float,
    val width: Float,
    val height: Float,
)

class BoundsTracker {
    val entries = LinkedHashMap<String, RecordedRect>()

    fun record(tag: String, x: Float, y: Float, w: Float, h: Float) {
        entries[tag] = RecordedRect(x, y, w, h)
    }
}

@Composable
internal fun Modifier.track(tag: String, tracker: BoundsTracker): Modifier {
    val density = LocalDensity.current.density
    return this.testTag(tag).onGloballyPositioned { coords ->
        val pos = coords.positionInRoot()
        val sz = coords.size
        tracker.record(
            tag = tag,
            x = pos.x / density,
            y = pos.y / density,
            w = sz.width / density,
            h = sz.height / density,
        )
    }
}

internal fun atlasTextStyle(
    fontFamily: FontFamily,
    sizeSp: Float,
    color: Color = INK,
    bold: Boolean = false,
    lineHeightEm: Float = 1.4f,
) = TextStyle(
    color = color,
    fontSize = sizeSp.sp,
    fontWeight = if (bold) FontWeight.Bold else FontWeight.Normal,
    fontFamily = fontFamily,
    lineHeight = lineHeightEm.em,
)

@Composable
internal fun ZoneCard(
    tag: String,
    title: String,
    accent: Color,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val shape = RoundedCornerShape(6.dp)
    Column(
        modifier
            .track(tag, tracker)
            .background(BG_CARD, shape)
            .border(1.dp, BORDER_CARD, shape)
            .padding(6.dp),
    ) {
        Row(
            Modifier.fillMaxWidth().height(15.dp),
            horizontalArrangement = Arrangement.spacedBy(5.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(4.dp, 11.dp).background(accent, RoundedCornerShape(2.dp)))
            BasicText(title, style = atlasTextStyle(fontFamily, 9f, INK, bold = true))
        }
        Spacer(Modifier.height(4.dp))
        content()
    }
}

@Composable
internal fun LabeledChip(
    tag: String,
    label: String,
    width: Dp,
    height: Dp,
    bg: Color,
    radius: Dp,
    fontSize: Float,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
) {
    val shape = if (radius.value > 0f) RoundedCornerShape(radius) else RectangleShape
    Box(
        Modifier
            .track(tag, tracker)
            .size(width, height)
            .background(bg, shape),
        contentAlignment = Alignment.Center,
    ) {
        BasicText(label, style = atlasTextStyle(fontFamily, fontSize, Color.White, bold = true))
    }
}

@Composable
internal fun FullWidthChip(
    tag: String,
    label: String,
    height: Dp,
    bg: Color,
    radius: Dp,
    fontSize: Float,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    align: Alignment = Alignment.Center,
    horizontalPadding: Dp = 0.dp,
) {
    val shape = if (radius.value > 0f) RoundedCornerShape(radius) else RectangleShape
    var mod = Modifier.track(tag, tracker).fillMaxWidth().height(height).background(bg, shape)
    if (horizontalPadding.value > 0f) {
        mod = mod.padding(horizontal = horizontalPadding)
    }
    Box(mod, contentAlignment = align) {
        BasicText(label, style = atlasTextStyle(fontFamily, fontSize, Color.White, bold = true))
    }
}

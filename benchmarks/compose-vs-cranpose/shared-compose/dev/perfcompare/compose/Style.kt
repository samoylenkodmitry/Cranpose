package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import dev.perfcompare.shared.CHIP_BACKGROUND_ARGB
import dev.perfcompare.shared.GRADIENT_END_ARGB
import dev.perfcompare.shared.PALETTE_ARGB

// What the Android app and the desktop app draw alike: the palettes, the text
// style over each platform's `Roboto`, and the top bar.

val PALETTE = PALETTE_ARGB.map { Color(it) }.toTypedArray()
val CHIP_BACKGROUND = CHIP_BACKGROUND_ARGB.map { Color(it) }.toTypedArray()
val GRADIENT_END = GRADIENT_END_ARGB.map { Color(it) }.toTypedArray()

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

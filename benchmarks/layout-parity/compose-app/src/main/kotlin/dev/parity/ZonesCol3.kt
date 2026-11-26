@file:OptIn(ExperimentalLayoutApi::class)

package dev.parity

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowColumn
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.absoluteOffset
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

@Composable
internal fun Zone9RtlLtrDirectionality(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z9_card", "9. RTL vs LTR Mirroring · Relative Padding/Offset · LTR Island", BLUE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Row(Modifier.fillMaxWidth().weight(1f), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                DirectionalColumnSample("ltr", LayoutDirection.Ltr, fontFamily, tracker, Modifier.weight(1f).fillMaxHeight())
                DirectionalColumnSample("rtl", LayoutDirection.Rtl, fontFamily, tracker, Modifier.weight(1f).fillMaxHeight())
            }
            CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl) {
                Row(
                    Modifier.track("z9_island_outer", tracker).fillMaxWidth().height(26.dp)
                        .background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape).padding(horizontal = 4.dp),
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    LabeledChip("z9_island_rtl_tag", "RTL-1", 38.dp, 18.dp, INDIGO, 0.dp, 8f, fontFamily, tracker)
                    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
                        Row(
                            Modifier.track("z9_island_ltr", tracker).background(BG_CARD)
                                .border(1.dp, BLUE, RectangleShape).padding(horizontal = 4.dp, vertical = 2.dp),
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            repeat(3) { i ->
                                LabeledChip("z9_isl_$i", "#$i", 24.dp, 14.dp, ACCENTS[i], 0.dp, 7.5f, fontFamily, tracker)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DirectionalColumnSample(
    prefix: String,
    direction: LayoutDirection,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    CompositionLocalProvider(LocalLayoutDirection provides direction) {
        Column(
            modifier.track("z9_${prefix}_panel", tracker).background(BG_SUBTLE)
                .border(1.dp, BORDER_STRONG, RectangleShape).padding(4.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            BasicText(prefix.uppercase(), style = atlasTextStyle(fontFamily, 8f, MUTED, bold = true))
            Row(
                Modifier.track("z9_${prefix}_row", tracker).fillMaxWidth().height(15.dp).background(BG_TRACK),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Box(Modifier.track("z9_${prefix}_r0", tracker).size(24.dp, 15.dp).background(BLUE))
                Box(Modifier.track("z9_${prefix}_r1", tracker).size(36.dp, 15.dp).background(EMERALD))
                Box(Modifier.track("z9_${prefix}_r2", tracker).size(16.dp, 15.dp).background(AMBER))
            }
            Box(Modifier.track("z9_${prefix}_col_bar", tracker).size(44.dp, 11.dp).background(VIOLET))
            Box(
                Modifier.track("z9_${prefix}_pad_outer", tracker).fillMaxWidth().height(19.dp)
                    .background(AMBER_SOFT).padding(start = 18.dp, top = 3.dp, end = 4.dp, bottom = 3.dp),
            ) {
                Box(Modifier.track("z9_${prefix}_pad_inner", tracker).fillMaxSize().background(AMBER))
            }
            Box(Modifier.track("z9_${prefix}_off_box", tracker).fillMaxWidth().height(30.dp).background(BG_TRACK)) {
                Box(Modifier.offset(x = 14.dp, y = 2.dp).track("z9_${prefix}_off_rel", tracker).size(28.dp, 11.dp).background(ROSE))
                Box(Modifier.absoluteOffset(x = 14.dp, y = 16.dp).track("z9_${prefix}_off_abs", tracker).size(28.dp, 11.dp).background(CYAN))
            }
            FlowRow(
                Modifier.track("z9_${prefix}_flow", tracker).fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                listOf(42.dp, 54.dp, 38.dp, 48.dp).forEachIndexed { i, w ->
                    Box(Modifier.track("z9_${prefix}_f$i", tracker).size(w, 12.dp).background(ACCENTS[i + 3]))
                }
            }
        }
    }
}

@Composable
internal fun Zone10FlowLayouts(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z10_card", "10. FlowRow & FlowColumn (Mixed Cross-Axis Sizes)", EMERALD, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FlowRow(
                Modifier.track("z10_flow_row", tracker).fillMaxWidth()
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape).padding(4.dp),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                val items = listOf(
                    50.dp to 14.dp, 66.dp to 22.dp, 42.dp to 12.dp,
                    78.dp to 16.dp, 56.dp to 20.dp, 72.dp to 14.dp,
                    46.dp to 24.dp, 60.dp to 14.dp, 52.dp to 16.dp,
                )
                items.forEachIndexed { i, (w, h) ->
                    LabeledChip("z10_fr_$i", "R$i", w, h, ACCENTS[i % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                }
            }

            FlowColumn(
                Modifier.track("z10_flow_col", tracker).fillMaxWidth().height(74.dp)
                    .background(TEAL_SOFT).border(1.dp, TEAL, RectangleShape).padding(4.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                val cols = listOf(
                    54.dp to 18.dp, 44.dp to 26.dp, 62.dp to 16.dp,
                    48.dp to 22.dp, 68.dp to 20.dp, 42.dp to 24.dp, 52.dp to 16.dp,
                )
                cols.forEachIndexed { i, (w, h) ->
                    LabeledChip("z10_fc_$i", "C$i", w, h, ACCENTS[(i + 2) % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
    }
}

@Composable
internal fun Zone11OffsetsAndGraphicsLayer(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z11_card", "11. Offsets (Static/Lambda/Split) · GraphicsLayer", VIOLET, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Zone11OffsetsRow(tracker)
            Zone11LayersRow(fontFamily, tracker)
            Zone11NestedLayersRow(fontFamily, tracker)
        }
    }
}

@Composable
private fun Zone11OffsetsRow(tracker: BoundsTracker) {
    val density = LocalDensity.current
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.track("z11_off_neg_p", tracker).padding(start = 6.dp).size(52.dp, 30.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape)) {
            Box(Modifier.offset((-6).dp, (-4).dp).track("z11_off_neg", tracker).size(28.dp, 18.dp).background(ROSE))
        }
        Box(Modifier.track("z11_off_clip_p", tracker).size(52.dp, 30.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape).clipToBounds()) {
            Box(Modifier.offset(30.dp, 14.dp).track("z11_off_clip", tracker).size(34.dp, 24.dp).background(EMERALD))
        }
        Box(Modifier.track("z11_off_lam_p", tracker).size(52.dp, 30.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape)) {
            Box(
                Modifier.offset { with(density) { IntOffset(10.dp.roundToPx(), 6.dp.roundToPx()) } }
                    .track("z11_off_lam", tracker).size(28.dp, 16.dp).background(BLUE),
            )
        }
        Box(Modifier.size(48.dp, 28.dp).background(CYAN_SOFT).offset(8.dp, 5.dp).track("z11_bg_off_bg", tracker).background(CYAN)) {
            Box(Modifier.track("z11_bg_off_inner", tracker).fillMaxSize())
        }
    }
}

@Composable
private fun Zone11LayersRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    val density = LocalDensity.current
    Row(
        Modifier.fillMaxWidth().padding(top = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.track("z11_gl_rot_c", tracker)
                .graphicsLayer { rotationZ = 14f; transformOrigin = TransformOrigin(0.5f, 0.5f) }
                .size(54.dp, 28.dp).background(INDIGO, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) { BasicText("Rot C", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true)) }

        Box(
            Modifier.track("z11_gl_rot_tl", tracker)
                .graphicsLayer { rotationZ = 14f; transformOrigin = TransformOrigin(0f, 0f) }
                .size(54.dp, 28.dp).background(VIOLET, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) { BasicText("Rot TL", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true)) }

        Box(
            Modifier.track("z11_gl_scale", tracker)
                .graphicsLayer { scaleX = 1.22f; scaleY = 0.78f; alpha = 0.55f }
                .size(52.dp, 28.dp).background(AMBER, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) { BasicText("Scale", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true)) }

        Box(
            Modifier.track("z11_gl_clip", tracker)
                .graphicsLayer {
                    translationX = with(density) { 6.dp.toPx() }
                    translationY = with(density) { (-4).dp.toPx() }
                    clip = true
                    shape = RoundedCornerShape(10.dp)
                }
                .size(54.dp, 28.dp).background(TEAL_SOFT).border(1.dp, TEAL, RoundedCornerShape(10.dp)),
            contentAlignment = Alignment.Center,
        ) { Box(Modifier.requiredSize(70.dp, 12.dp).background(TEAL)) }
    }
}

@Composable
private fun Zone11NestedLayersRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    val density = LocalDensity.current
    Row(
        Modifier.fillMaxWidth().padding(top = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(24.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.track("z11_gl_nest_p", tracker).padding(start = 8.dp)
                .graphicsLayer { rotationZ = 10f }
                .size(96.dp, 28.dp).background(BLUE_SOFT, RoundedCornerShape(4.dp))
                .border(1.dp, BLUE, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier.track("z11_gl_nest_c", tracker)
                    .graphicsLayer { rotationZ = -10f }
                    .size(64.dp, 16.dp).background(BLUE, RoundedCornerShape(3.dp)),
                contentAlignment = Alignment.Center,
            ) { BasicText("Level 0°", style = atlasTextStyle(fontFamily, 7.5f, Color.White, bold = true)) }
        }

        Box(
            Modifier.track("z11_gl_combo", tracker)
                .graphicsLayer {
                    rotationZ = -10f
                    scaleX = 1.15f
                    translationX = with(density) { 6.dp.toPx() }
                    transformOrigin = TransformOrigin(1f, 0.5f)
                }
                .size(62.dp, 24.dp).background(ROSE, RoundedCornerShape(4.dp)),
            contentAlignment = Alignment.Center,
        ) { BasicText("Pivot R", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true)) }
    }
}

@Composable
internal fun Zone12DrawModifiersAndShapes(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z12_card", "12. drawBehind · drawWithContent · drawWithCache · Shapes", ROSE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Zone12DrawRow(fontFamily, tracker)
            Zone12ShapeRow(fontFamily, tracker)
            Zone12GradientRow(fontFamily, tracker)
        }
    }
}

@Composable
private fun Zone12DrawRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(
            Modifier.weight(1f).track("z12_draw_behind", tracker).height(42.dp).drawBehind {
                drawRoundRect(BLUE_SOFT, cornerRadius = CornerRadius(6.dp.toPx()))
                drawCircle(Color(0x552563EB), radius = 14.dp.toPx(), center = Offset(size.width * 0.25f, size.height * 0.5f))
                drawLine(BLUE, Offset(4.dp.toPx(), size.height - 4.dp.toPx()), Offset(size.width - 4.dp.toPx(), 4.dp.toPx()), strokeWidth = 1.5.dp.toPx())
            },
            contentAlignment = Alignment.Center,
        ) { BasicText("Behind", style = atlasTextStyle(fontFamily, 8.5f, INK, bold = true)) }

        Box(
            Modifier.weight(1f).track("z12_draw_content", tracker).height(42.dp).drawWithContent {
                drawRoundRect(AMBER_SOFT, cornerRadius = CornerRadius(6.dp.toPx()))
                drawContent()
                drawLine(Color(0xBBDC2626), Offset(4.dp.toPx(), 4.dp.toPx()), Offset(size.width - 4.dp.toPx(), size.height - 4.dp.toPx()), strokeWidth = 2.dp.toPx())
                drawCircle(RED, radius = 4.dp.toPx(), center = Offset(size.width - 8.dp.toPx(), 8.dp.toPx()))
            },
            contentAlignment = Alignment.Center,
        ) { BasicText("WithContent", style = atlasTextStyle(fontFamily, 8.5f, INK, bold = true)) }

        Box(
            Modifier.weight(1f).track("z12_draw_cache", tracker).height(42.dp).drawWithCache {
                val rOuter = CornerRadius(8.dp.toPx())
                val rInner = CornerRadius(4.dp.toPx())
                val inset = 4.dp.toPx()
                val strokeW = 1.5.dp.toPx()
                onDrawBehind {
                    drawRoundRect(EMERALD_SOFT, cornerRadius = rOuter)
                    drawRoundRect(
                        EMERALD,
                        topLeft = Offset(inset, inset),
                        size = Size(size.width - inset * 2, size.height - inset * 2),
                        cornerRadius = rInner,
                        style = Stroke(width = strokeW),
                    )
                }
            },
            contentAlignment = Alignment.Center,
        ) { BasicText("Cache", style = atlasTextStyle(fontFamily, 8.5f, EMERALD, bold = true)) }
    }
}

@Composable
private fun Zone12ShapeRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        val asym = RoundedCornerShape(topStart = 14.dp, topEnd = 2.dp, bottomEnd = 14.dp, bottomStart = 2.dp)
        Box(
            Modifier.weight(1f).track("z12_asym", tracker).height(34.dp).background(VIOLET_SOFT, asym).border(2.dp, VIOLET, asym),
            contentAlignment = Alignment.Center,
        ) { BasicText("Asym 14/2", style = atlasTextStyle(fontFamily, 8f, VIOLET, bold = true)) }

        val pill = RoundedCornerShape(50.dp)
        Box(
            Modifier.weight(1f).track("z12_pill", tracker).height(34.dp).background(CYAN_SOFT, pill).border(2.dp, CYAN, pill),
            contentAlignment = Alignment.Center,
        ) { BasicText("Clamp Pill", style = atlasTextStyle(fontFamily, 8f, CYAN, bold = true)) }

        val sh = RoundedCornerShape(8.dp)
        Box(
            Modifier.weight(1f).track("z12_shadow", tracker).height(34.dp).shadow(4.dp, sh).background(BG_CARD, sh).border(1.dp, BORDER_CARD, sh),
            contentAlignment = Alignment.Center,
        ) { BasicText("Shadow 4", style = atlasTextStyle(fontFamily, 8f, INK, bold = true)) }
    }
}

@Composable
private fun Zone12GradientRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.weight(1f).track("z12_grad_h", tracker).height(28.dp).drawBehind {
                drawRoundRect(
                    brush = Brush.horizontalGradient(listOf(BLUE, CYAN, EMERALD)),
                    cornerRadius = CornerRadius(6.dp.toPx()),
                )
            },
            contentAlignment = Alignment.Center,
        ) { BasicText("H-Grad", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true)) }

        val gShape = RoundedCornerShape(6.dp)
        Box(
            Modifier.weight(1f).track("z12_grad_v", tracker).height(28.dp)
                .background(BG_CARD, gShape).border(2.dp, Brush.verticalGradient(listOf(ROSE, AMBER)), gShape),
            contentAlignment = Alignment.Center,
        ) { BasicText("V-Border", style = atlasTextStyle(fontFamily, 8f, INK, bold = true)) }

        Box(
            Modifier.weight(1f).track("z12_arc", tracker).height(28.dp).drawBehind {
                drawRect(BG_SUBTLE)
                drawRect(BORDER_STRONG, style = Stroke(width = 1.dp.toPx()))
                val r = 10.dp.toPx()
                drawArc(
                    color = INDIGO,
                    startAngle = -90f,
                    sweepAngle = 240f,
                    useCenter = false,
                    topLeft = Offset(size.width * 0.5f - r, size.height * 0.5f - r),
                    size = Size(r * 2f, r * 2f),
                    style = Stroke(width = 3.dp.toPx()),
                )
            },
        )
    }
}

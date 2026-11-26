package dev.parity

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.IntrinsicMeasurable
import androidx.compose.ui.layout.IntrinsicMeasureScope
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasurePolicy
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.layout.layout
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.constrainHeight
import androidx.compose.ui.unit.constrainWidth
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.roundToInt

@Composable
internal fun Zone5Intrinsics(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z5_card", "5. Intrinsics (Min/Max Height & Width · Nested)", VIOLET, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(
                Modifier.track("z5_imin_row", tracker).fillMaxWidth().height(IntrinsicSize.Min)
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape).padding(4.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                BasicText(
                    "IntrinsicSize.Min stretches the red divider to match wrapped text height.",
                    Modifier.weight(1f).track("z5_imin_left", tracker),
                    style = atlasTextStyle(fontFamily, 8.5f, INK),
                )
                Box(Modifier.track("z5_imin_div", tracker).width(3.dp).fillMaxHeight().background(RED))
                BasicText(
                    "Centered",
                    Modifier.weight(0.55f).align(Alignment.CenterVertically).track("z5_imin_right", tracker),
                    style = atlasTextStyle(fontFamily, 8.5f, VIOLET, bold = true),
                )
            }

            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Column(
                    Modifier.track("z5_wmax_col", tracker).width(IntrinsicSize.Max)
                        .background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape).padding(4.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    BasicText("Hi", style = atlasTextStyle(fontFamily, 8.5f, INK))
                    BasicText("Widest label here", style = atlasTextStyle(fontFamily, 8.5f, BLUE, bold = true))
                    Box(Modifier.track("z5_wmax_bar", tracker).fillMaxWidth().height(5.dp).background(BLUE))
                }

                Column(
                    Modifier.track("z5_wmin_col", tracker).width(IntrinsicSize.Min)
                        .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape).padding(4.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    BasicText("Min wraps words", Modifier.track("z5_wmin_txt", tracker), style = atlasTextStyle(fontFamily, 8.5f, INK))
                    Box(Modifier.track("z5_wmin_bar", tracker).fillMaxWidth().height(5.dp).background(AMBER))
                }
            }

            Row(
                Modifier.track("z5_hmax_row", tracker).fillMaxWidth().height(IntrinsicSize.Max)
                    .background(EMERALD_SOFT).border(1.dp, EMERALD, RectangleShape).padding(4.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Box(Modifier.track("z5_hmax_p1", tracker).size(56.dp, 12.dp).background(EMERALD))
                    Box(Modifier.track("z5_hmax_p2", tracker).size(42.dp, 15.dp).background(TEAL))
                }
                Box(Modifier.track("z5_hmax_bar", tracker).width(6.dp).fillMaxHeight().background(INK))
                Box(Modifier.weight(1f).track("z5_hmax_fill", tracker).fillMaxHeight().background(Color(0x55059669)))
            }
        }
    }
}

@Composable
internal fun Zone6BaselinesAndText(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z6_card", "6. alignByBaseline · Custom Line · Text Wrapping/Ellipsis", ROSE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Zone6BaselineRow(fontFamily, tracker)
            Zone6TextOverflowRow(fontFamily, tracker)
            Zone6AnnotatedAndAlign(fontFamily, tracker)
        }
    }
}

@Composable
private fun Zone6BaselineRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(
        Modifier.track("z6_base_row", tracker).fillMaxWidth()
            .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape).padding(4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        BasicText("Ag", Modifier.alignByBaseline().track("z6_base_big", tracker), style = atlasTextStyle(fontFamily, 22f, INK, bold = true))
        BasicText("base", Modifier.alignByBaseline().track("z6_base_small", tracker), style = atlasTextStyle(fontFamily, 9.5f, MUTED))
        BasicText("42", Modifier.alignByBaseline().track("z6_base_med", tracker), style = atlasTextStyle(fontFamily, 14f, BLUE, bold = true))
        Box(Modifier.track("z6_base_nobase", tracker).size(14.dp, 14.dp).background(AMBER))
        BasicText(
            "Wrap\nline2",
            Modifier.width(36.dp).alignByBaseline().track("z6_base_wrap", tracker),
            style = atlasTextStyle(fontFamily, 9f, EMERALD),
        )
        Layout(
            content = {},
            modifier = Modifier.alignByBaseline().track("z6_base_custom", tracker).size(28.dp, 24.dp)
                .background(RED_SOFT).drawBehind {
                    val y = 18.dp.toPx()
                    drawLine(RED, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.5.dp.toPx())
                },
        ) { _, constraints ->
            val w = constraints.constrainWidth(28.dp.roundToPx())
            val h = constraints.constrainHeight(24.dp.roundToPx())
            layout(w, h, mapOf(FirstBaseline to 18.dp.roundToPx())) {}
        }
    }
}

@Composable
private fun Zone6TextOverflowRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Box(Modifier.weight(1f).track("z6_txt_ellipsis_box", tracker).background(BLUE_SOFT).padding(3.dp)) {
            BasicText(
                "Ellipsis clamps this longer multi-line paragraph cleanly at line two with dots.",
                Modifier.track("z6_txt_ellipsis", tracker),
                style = atlasTextStyle(fontFamily, 8.5f, INK),
                overflow = TextOverflow.Ellipsis,
                maxLines = 2,
            )
        }
        Box(Modifier.weight(0.8f).track("z6_txt_minlines_box", tracker).background(AMBER_SOFT).padding(3.dp)) {
            BasicText(
                "Min 3 lines",
                Modifier.track("z6_txt_minlines", tracker),
                style = atlasTextStyle(fontFamily, 8.5f, INK),
                minLines = 3,
            )
        }
        Box(Modifier.weight(0.9f).track("z6_txt_nowrap_box", tracker).background(ROSE_SOFT).clipToBounds().padding(3.dp)) {
            BasicText(
                "NoWrap clips right past edge",
                Modifier.track("z6_txt_nowrap", tracker),
                style = atlasTextStyle(fontFamily, 8.5f, ROSE),
                softWrap = false,
                overflow = TextOverflow.Clip,
            )
        }
    }
}

@Composable
private fun Zone6AnnotatedAndAlign(fontFamily: FontFamily, tracker: BoundsTracker) {
    val rich = buildAnnotatedString {
        withStyle(SpanStyle(color = VIOLET, fontSize = 12.sp, fontWeight = FontWeight.Bold)) { append("Rich ") }
        withStyle(SpanStyle(color = INK, fontSize = 9.sp)) { append("annotated spans ") }
        withStyle(SpanStyle(color = RED, fontSize = 10.5.sp, fontWeight = FontWeight.Bold)) { append("1.4em") }
    }
    BasicText(
        rich,
        Modifier.track("z6_txt_annotated", tracker).fillMaxWidth()
            .background(VIOLET_SOFT).padding(horizontal = 4.dp, vertical = 2.dp),
        style = atlasTextStyle(fontFamily, 9.5f, INK),
    )

    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        val aligns = listOf(
            Triple("z6_ta_start", "Start", Pair(INK, TextAlign.Start)),
            Triple("z6_ta_center", "Center", Pair(BLUE, TextAlign.Center)),
            Triple("z6_ta_end", "End", Pair(ROSE, TextAlign.End)),
        )
        aligns.forEachIndexed { idx, (tag, text, spec) ->
            BasicText(
                text,
                Modifier.weight(1f).track(tag, tracker).background(BG_TRACK).padding(horizontal = 4.dp, vertical = 2.dp),
                style = atlasTextStyle(fontFamily, 8.5f, spec.first, bold = idx > 0).copy(textAlign = spec.second),
            )
        }
    }
}

private fun Modifier.haloPaddingLayout(padX: Dp, padY: Dp): Modifier = this.layout { m, c ->
    val dx = padX.roundToPx()
    val dy = padY.roundToPx()
    val p = m.measure(c.copy(minWidth = 0, minHeight = 0))
    layout(c.constrainWidth(p.width + dx * 2), c.constrainHeight(p.height + dy * 2)) {
        p.placeRelative(dx, dy)
    }
}

private fun diagonalMeasurePolicy(): MeasurePolicy = object : MeasurePolicy {
    override fun MeasureScope.measure(measurables: List<Measurable>, constraints: Constraints): MeasureResult {
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val placeables = measurables.map { it.measure(loose) }
        val stepX = 22.dp.roundToPx()
        val stepY = 11.dp.roundToPx()
        var maxR = 0
        var maxB = 0
        placeables.forEachIndexed { i, p ->
            maxR = max(maxR, i * stepX + p.width)
            maxB = max(maxB, i * stepY + p.height)
        }
        return layout(constraints.constrainWidth(maxR), constraints.constrainHeight(maxB)) {
            placeables.forEachIndexed { i, p -> p.placeRelative(i * stepX, i * stepY) }
        }
    }

    override fun IntrinsicMeasureScope.minIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int) =
        (measurables.size * 22).dp.roundToPx()

    override fun IntrinsicMeasureScope.maxIntrinsicWidth(measurables: List<IntrinsicMeasurable>, height: Int) =
        (measurables.size * 22 + 38).dp.roundToPx()

    override fun IntrinsicMeasureScope.minIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int) =
        (measurables.size * 11 + 16).dp.roundToPx()

    override fun IntrinsicMeasureScope.maxIntrinsicHeight(measurables: List<IntrinsicMeasurable>, width: Int) =
        (measurables.size * 11 + 16).dp.roundToPx()
}

@Composable
internal fun Zone7CustomLayoutAndBwc(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z7_card", "7. Custom Layout · Modifier.layout · BoxWithConstraints", EMERALD, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Zone7TopRow(fontFamily, tracker)
            Zone7BwcArea(fontFamily, tracker, Modifier.fillMaxWidth().weight(1f))
        }
    }
}

@Composable
private fun Zone7TopRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Layout(
            content = {
                repeat(4) { i ->
                    LabeledChip("z7_diag_$i", "D$i", 38.dp, 16.dp, ACCENTS[i], 0.dp, 8f, fontFamily, tracker)
                }
            },
            modifier = Modifier.track("z7_custom_layout", tracker)
                .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
            measurePolicy = diagonalMeasurePolicy(),
        )

        Box(
            Modifier.weight(1f).track("z7_mod_layout_outer", tracker)
                .background(CYAN_SOFT).border(1.dp, CYAN, RectangleShape)
                .layout { measurable, constraints ->
                    val padX = 12.dp.roundToPx()
                    val padY = 6.dp.roundToPx()
                    val innerMaxW = (constraints.maxWidth - padX * 2).coerceAtLeast(0)
                    val innerMaxH = if (constraints.hasBoundedHeight) (constraints.maxHeight - padY * 2).coerceAtLeast(0) else Constraints.Infinity
                    val p = measurable.measure(constraints.copy(minWidth = 0, maxWidth = innerMaxW, minHeight = 0, maxHeight = innerMaxH))
                    layout(constraints.constrainWidth(p.width + padX * 2), constraints.constrainHeight(p.height + padY * 2)) {
                        p.placeRelative(padX, padY)
                    }
                },
        ) {
            Row(
                Modifier.track("z7_mod_layout_inner", tracker).fillMaxWidth().height(26.dp).background(CYAN),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                BasicText("Mod.layout", style = atlasTextStyle(fontFamily, 8.5f, Color.White, bold = true))
            }
        }
    }
}

@Composable
private fun Zone7BwcArea(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    BoxWithConstraints(
        modifier.track("z7_bwc", tracker).background(EMERALD_SOFT)
            .border(1.dp, EMERALD, RectangleShape).padding(4.dp),
    ) {
        val mw = maxWidth
        val chips = if (mw >= 220.dp) 3 else 2
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Box(
                Modifier.track("z7_bwc_bar", tracker).width(mw * 0.62f).height(14.dp).background(EMERALD),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicText(" 62% of ${mw.value.roundToInt()}dp", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                repeat(chips) { i ->
                    LabeledChip("z7_bwc_c$i", "BWC $i", 56.dp, 18.dp, TEAL, 0.dp, 8f, fontFamily, tracker)
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                BoxWithConstraints(
                    Modifier.weight(1f).track("z7_bwc_nested", tracker).height(26.dp)
                        .background(BG_CARD).border(1.dp, TEAL, RectangleShape).padding(3.dp),
                ) {
                    Box(Modifier.track("z7_bwc_nested_bar", tracker).width(maxWidth * 0.45f).fillMaxHeight().background(INDIGO))
                }
                Box(
                    Modifier.track("z7_mod_chain_outer", tracker)
                        .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape)
                        .haloPaddingLayout(5.dp, 2.dp)
                        .haloPaddingLayout(4.dp, 3.dp),
                ) {
                    LabeledChip("z7_mod_chain_inner", "2xLayout", 58.dp, 16.dp, AMBER, 0.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
    }
}

@Composable
internal fun Zone8SubcomposeLayout(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z8_card", "8. SubcomposeLayout (Two-Pass Dependent & Dynamic Slots)", AMBER, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Zone8TwoPassSubcompose(fontFamily, tracker)
            Zone8DynamicSlotSubcompose(fontFamily, tracker)
        }
    }
}

@Composable
private fun Zone8TwoPassSubcompose(fontFamily: FontFamily, tracker: BoundsTracker) {
    SubcomposeLayout(
        Modifier.track("z8_subcompose", tracker).fillMaxWidth().height(68.dp)
            .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
    ) { constraints ->
        val padPx = 4.dp.roundToPx()
        val gapPx = 6.dp.roundToPx()
        val innerW = (constraints.maxWidth - padPx * 2).coerceAtLeast(0)
        val loose = Constraints(0, innerW, 0, constraints.maxHeight)

        val anchorPlaceables = subcompose("anchor") {
            Box(
                Modifier.track("z8_sub_anchor", tracker)
                    .background(AMBER, RoundedCornerShape(4.dp)).padding(horizontal = 8.dp, vertical = 3.dp),
            ) {
                BasicText("Subcomposed Anchor", style = atlasTextStyle(fontFamily, 8.5f, Color.White, bold = true))
            }
        }.map { it.measure(loose) }
        val anchorW = anchorPlaceables.maxOfOrNull { it.width } ?: 0
        val anchorH = anchorPlaceables.maxOfOrNull { it.height } ?: 0
        val remW = (innerW - anchorW - gapPx).coerceAtLeast(0)
        val halfW = (anchorW * 0.5f).roundToInt()
        val restW = (innerW - halfW - gapPx).coerceAtLeast(0)

        val trackPlaceables = subcompose("tracks") {
            Box(Modifier.track("z8_sub_under", tracker).size(anchorW.toDp(), 6.dp).background(RED))
            Box(Modifier.track("z8_sub_rem", tracker).size(remW.toDp(), anchorH.toDp()).background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape))
            Box(Modifier.track("z8_sub_bot_l", tracker).size(halfW.toDp(), 14.dp).background(VIOLET))
            Box(Modifier.track("z8_sub_bot_r", tracker).size(restW.toDp(), 14.dp).background(TEAL))
        }.map { it.measure(loose) }

        layout(constraints.maxWidth, constraints.maxHeight) {
            anchorPlaceables.forEach { it.placeRelative(padPx, padPx) }
            if (trackPlaceables.size >= 4) {
                trackPlaceables[0].placeRelative(padPx, padPx + anchorH + 3.dp.roundToPx())
                trackPlaceables[1].placeRelative(padPx + anchorW + gapPx, padPx)
                val botY = padPx + anchorH + 14.dp.roundToPx()
                trackPlaceables[2].placeRelative(padPx, botY)
                trackPlaceables[3].placeRelative(padPx + halfW + gapPx, botY)
            }
        }
    }
}

@Composable
private fun Zone8DynamicSlotSubcompose(fontFamily: FontFamily, tracker: BoundsTracker) {
    SubcomposeLayout(
        Modifier.track("z8_sub_dyn", tracker).fillMaxWidth().height(34.dp)
            .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape),
    ) { constraints ->
        val padPx = 4.dp.roundToPx()
        val tileW = 36.dp.roundToPx()
        val tileH = 22.dp.roundToPx()
        val gapPx = 5.dp.roundToPx()
        val usable = (constraints.maxWidth - padPx * 2).coerceAtLeast(0)
        val count = floor((usable + gapPx).toFloat() / (tileW + gapPx).toFloat()).toInt().coerceAtLeast(1)
        val placed = (0 until count).map { i ->
            val ms = subcompose(i) {
                LabeledChip("z8_sub_t$i", "S$i", 36.dp, 22.dp, ACCENTS[i % ACCENTS.size], 0.dp, 8f, fontFamily, tracker)
            }
            ms.first().measure(Constraints.fixed(tileW, tileH))
        }
        val topY = ((constraints.maxHeight - tileH) / 2).coerceAtLeast(0)
        layout(constraints.maxWidth, constraints.maxHeight) {
            placed.forEachIndexed { i, p -> p.placeRelative(padPx + i * (tileW + gapPx), topY) }
        }
    }
}

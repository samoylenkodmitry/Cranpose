package dev.parity

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.requiredWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex

@Composable
internal fun Zone1ModifierChainsAndConstraints(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z1_card", "1. Modifier Chain · Coercion · requiredSize · wrapContent", BLUE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Zone1RowA(fontFamily, tracker)
            Zone1RowB(tracker)
            Zone1RowC(fontFamily, tracker)
            Zone1RowD(tracker)
        }
    }
}

@Composable
private fun Zone1RowA(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .track("z1_chain_box", tracker)
                .padding(4.dp)
                .background(BLUE_SOFT)
                .border(1.dp, BLUE, RectangleShape)
                .padding(5.dp)
                .background(AMBER_SOFT)
                .border(1.dp, AMBER, RectangleShape)
                .padding(4.dp)
                .background(EMERALD_SOFT)
                .size(64.dp, 22.dp),
            contentAlignment = Alignment.Center,
        ) {
            BasicText("Chain", Modifier.track("z1_chain_inner", tracker), style = atlasTextStyle(fontFamily, 8.5f, INK, bold = true))
        }

        Box(
            Modifier
                .track("z1_outer_wins", tracker)
                .size(74.dp, 28.dp)
                .size(36.dp, 14.dp)
                .background(INDIGO_SOFT)
                .border(1.dp, INDIGO, RectangleShape),
            contentAlignment = Alignment.Center,
        ) {
            BasicText("Outer74", style = atlasTextStyle(fontFamily, 8f, INDIGO, bold = true))
        }

        Box(
            Modifier
                .track("z1_req_parent", tracker)
                .size(46.dp, 22.dp)
                .background(RED_SOFT)
                .border(1.dp, RED, RectangleShape),
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier
                    .track("z1_req_child", tracker)
                    .requiredSize(62.dp, 30.dp)
                    .background(Color(0x66DC2626))
                    .border(1.dp, RED, RectangleShape),
                contentAlignment = Alignment.Center,
            ) {
                BasicText("Req62", style = atlasTextStyle(fontFamily, 7.5f, INK, bold = true))
            }
        }
    }
}

@Composable
private fun Zone1RowB(tracker: BoundsTracker) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .track("z1_sizein_min", tracker)
                .sizeIn(minWidth = 58.dp, maxWidth = 90.dp, minHeight = 22.dp, maxHeight = 32.dp)
                .background(VIOLET_SOFT)
                .border(1.dp, VIOLET, RectangleShape),
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.track("z1_sizein_min_c", tracker).size(16.dp, 10.dp).background(VIOLET))
        }

        Box(
            Modifier
                .track("z1_sizein_max", tracker)
                .sizeIn(minWidth = 20.dp, maxWidth = 52.dp, minHeight = 14.dp, maxHeight = 24.dp)
                .background(TEAL_SOFT)
                .border(1.dp, TEAL, RectangleShape),
            contentAlignment = Alignment.TopStart,
        ) {
            Box(Modifier.track("z1_sizein_max_c", tracker).size(90.dp, 60.dp).background(Color(0x880D9488)))
        }

        Box(
            Modifier
                .weight(1f)
                .track("z1_frac_parent", tracker)
                .height(34.dp)
                .background(BG_SUBTLE)
                .border(1.dp, BORDER_STRONG, RectangleShape),
        ) {
            Box(
                Modifier
                    .align(Alignment.CenterEnd)
                    .track("z1_frac_child", tracker)
                    .fillMaxWidth(0.37f)
                    .fillMaxHeight(0.65f)
                    .background(CYAN),
            )
        }
    }
}

@Composable
private fun UnboundedWrapBox(
    outerTag: String,
    childTag: String,
    startPad: Dp,
    clip: Boolean,
    fill: Color,
    stroke: Color,
    tracker: BoundsTracker,
) {
    var outer = Modifier.track(outerTag, tracker)
    if (startPad.value > 0f) outer = outer.padding(start = startPad)
    outer = outer.size(58.dp, 28.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape)
    if (clip) outer = outer.clipToBounds()
    Box(outer) {
        Box(Modifier.fillMaxSize().wrapContentSize(Alignment.BottomEnd, unbounded = true)) {
            Box(Modifier.track(childTag, tracker).size(72.dp, 36.dp).background(fill).border(1.dp, stroke, RectangleShape))
        }
    }
}

@Composable
private fun Zone1RowC(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(
        Modifier.fillMaxWidth().padding(top = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(26.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        UnboundedWrapBox("z1_wrap_unclipped", "z1_wrap_child_a", 16.dp, false, Color(0x667C3AED), VIOLET, tracker)
        UnboundedWrapBox("z1_wrap_clipped", "z1_wrap_child_b", 0.dp, true, Color(0x66059669), EMERALD, tracker)
        BasicText("unbounded:\nunclip vs clip", style = atlasTextStyle(fontFamily, 8f, MUTED))
    }
}

@Composable
private fun Zone1RowD(tracker: BoundsTracker) {
    Row(
        Modifier.fillMaxWidth().padding(top = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.track("z1_chain_fill_p", tracker).size(96.dp, 20.dp).background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
        ) {
            Box(Modifier.track("z1_chain_fill", tracker).fillMaxWidth(0.5f).fillMaxWidth(0.5f).fillMaxHeight().background(BLUE))
        }

        Box(
            Modifier.track("z1_req_w_p", tracker).size(46.dp, 20.dp).background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape),
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier.track("z1_req_w_only", tracker).requiredWidth(72.dp).fillMaxHeight()
                    .background(Color(0x66D97706)).border(1.dp, AMBER, RectangleShape),
            )
        }

        Box(
            Modifier.track("z1_wh_in", tracker).widthIn(min = 58.dp).heightIn(max = 18.dp)
                .background(EMERALD_SOFT).border(1.dp, EMERALD, RectangleShape),
        ) {
            Box(Modifier.track("z1_wh_in_c", tracker).size(24.dp, 38.dp).background(EMERALD))
        }
    }
}

@Composable
internal fun Zone2BoxAlignmentAndZIndex(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z2_card", "2. Box 9-Align · propagateMinConstraints · zIndex", INDIGO, fontFamily, tracker, modifier) {
        Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(
                Modifier.weight(1.12f).track("z2_align9", tracker).fillMaxHeight()
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
            ) {
                val points = listOf(
                    Triple("z2_a_ts", Alignment.TopStart, Pair(19.dp, 13.dp)),
                    Triple("z2_a_tc", Alignment.TopCenter, Pair(23.dp, 11.dp)),
                    Triple("z2_a_te", Alignment.TopEnd, Pair(17.dp, 15.dp)),
                    Triple("z2_a_cs", Alignment.CenterStart, Pair(21.dp, 13.dp)),
                    Triple("z2_a_cc", Alignment.Center, Pair(29.dp, 19.dp)),
                    Triple("z2_a_ce", Alignment.CenterEnd, Pair(15.dp, 17.dp)),
                    Triple("z2_a_bs", Alignment.BottomStart, Pair(25.dp, 11.dp)),
                    Triple("z2_a_bc", Alignment.BottomCenter, Pair(19.dp, 15.dp)),
                    Triple("z2_a_be", Alignment.BottomEnd, Pair(23.dp, 13.dp)),
                )
                points.forEachIndexed { idx, (tag, align, sz) ->
                    Box(Modifier.align(align).track(tag, tracker).size(sz.first, sz.second).background(ACCENTS[idx % ACCENTS.size]))
                }
            }

            Column(Modifier.weight(0.95f).fillMaxHeight(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    PropMinBox("z2_prop_false", "z2_prop_false_c", false, AMBER_SOFT, AMBER, AMBER, tracker)
                    PropMinBox("z2_prop_true", "z2_prop_true_c", true, EMERALD_SOFT, EMERALD, Color(0x99059669), tracker)
                }
                Zone2ZStack(fontFamily, tracker, Modifier.fillMaxWidth().weight(1f))
            }
        }
    }
}

@Composable
private fun PropMinBox(
    outerTag: String,
    childTag: String,
    propagate: Boolean,
    bg: Color,
    stroke: Color,
    childBg: Color,
    tracker: BoundsTracker,
) {
    Box(
        Modifier.track(outerTag, tracker).sizeIn(minWidth = 68.dp, minHeight = 28.dp)
            .background(bg).border(1.dp, stroke, RectangleShape),
        contentAlignment = Alignment.Center,
        propagateMinConstraints = propagate,
    ) {
        Box(Modifier.track(childTag, tracker).size(20.dp, 12.dp).background(childBg))
    }
}

@Composable
private fun Zone2ZStack(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    data class ZLayer(val tag: String, val label: String, val z: Float, val x: Dp, val y: Dp, val bg: Color)
    val layers = listOf(
        ZLayer("z2_z_2", "z=2", 2f, 4.dp, 4.dp, RED),
        ZLayer("z2_z_neg1", "z=-1", -1f, 24.dp, 12.dp, BLUE),
        ZLayer("z2_z_05a", "z=.5a", 0.5f, 44.dp, 8.dp, EMERALD),
        ZLayer("z2_z_05b", "z=.5b", 0.5f, 64.dp, 20.dp, AMBER),
    )
    Box(
        modifier.track("z2_zstack", tracker).background(BG_SUBTLE)
            .border(1.dp, BORDER_STRONG, RectangleShape).padding(4.dp),
    ) {
        layers.forEach { layer ->
            Box(
                Modifier.zIndex(layer.z).offset(layer.x, layer.y).track(layer.tag, tracker)
                    .size(50.dp, 28.dp).background(layer.bg).border(1.dp, Color.White, RectangleShape),
                contentAlignment = Alignment.Center,
            ) {
                BasicText(layer.label, style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true))
            }
        }
    }
}

@Composable
internal fun Zone3ArrangementsAndOverflow(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z3_card", "3. All Arrangements · SpacedByAligned · Overflow Clamp", CYAN, fontFamily, tracker, modifier) {
        Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Column(Modifier.weight(1.75f).fillMaxHeight(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                val specs = listOf(
                    "start" to Arrangement.Start,
                    "center" to Arrangement.Center,
                    "end" to Arrangement.End,
                    "between" to Arrangement.SpaceBetween,
                    "around" to Arrangement.SpaceAround,
                    "evenly" to Arrangement.SpaceEvenly,
                    "sp_end" to Arrangement.spacedBy(7.dp, Alignment.End),
                )
                val widths = listOf(26.dp, 38.dp, 20.dp)
                val colors = listOf(BLUE, EMERALD, AMBER)
                specs.forEach { (name, arr) ->
                    Row(
                        Modifier.track("z3_$name", tracker).fillMaxWidth().height(17.dp)
                            .background(BG_TRACK).padding(vertical = 2.dp),
                        horizontalArrangement = arr,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        widths.forEachIndexed { i, w ->
                            Box(Modifier.track("z3_${name}_$i", tracker).size(w, 13.dp).background(colors[i]))
                        }
                    }
                }
                Row(
                    Modifier.track("z3_overflow", tracker).fillMaxWidth().height(19.dp)
                        .background(RED_SOFT).border(1.dp, RED, RectangleShape)
                        .clipToBounds().padding(vertical = 2.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    repeat(5) { i ->
                        Box(Modifier.track("z3_ov_$i", tracker).size(54.dp, 13.dp).background(ACCENTS[i]))
                    }
                }
            }

            Column(
                Modifier.weight(0.60f).track("z3_col", tracker).fillMaxHeight()
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape).padding(horizontal = 3.dp),
                verticalArrangement = Arrangement.SpaceEvenly,
            ) {
                val aligns = listOf(Alignment.Start, Alignment.CenterHorizontally, Alignment.End, Alignment.CenterHorizontally)
                aligns.forEachIndexed { i, al ->
                    Box(Modifier.align(al).track("z3_col_$i", tracker).size((24 + i * 5).dp, 14.dp).background(ACCENTS[i + 2]))
                }
            }
        }
    }
}

@Composable
internal fun Zone4WeightsFillAndMixed(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z4_card", "4. Weights (fill=true/false) · Mixed Fixed · Tight Remainder", TEAL, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Row(
                Modifier.track("z4_a", tracker).fillMaxWidth().height(24.dp).background(BG_TRACK).padding(2.dp),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Box(Modifier.track("z4_a_f1", tracker).width(34.dp).fillMaxHeight().background(SLATE))
                Box(Modifier.weight(1f, fill = true).track("z4_a_w1", tracker).fillMaxHeight().background(BLUE))
                Box(Modifier.weight(2.5f, fill = true).track("z4_a_w2", tracker).fillMaxHeight().background(EMERALD))
                Box(Modifier.track("z4_a_f2", tracker).width(26.dp).fillMaxHeight().background(SLATE))
            }

            Row(
                Modifier.track("z4_b", tracker).fillMaxWidth().height(24.dp).background(BG_TRACK).padding(2.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Box(Modifier.weight(1f, fill = false).track("z4_b_nf1", tracker).size(24.dp, 20.dp).background(AMBER))
                Box(Modifier.weight(1f, fill = true).track("z4_b_f", tracker).fillMaxHeight().background(VIOLET))
                Box(Modifier.weight(2f, fill = false).track("z4_b_nf2", tracker).size(34.dp, 20.dp).background(ROSE))
            }

            Row(
                Modifier.track("z4_c", tracker).fillMaxWidth().height(22.dp).background(BG_TRACK).padding(2.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Box(Modifier.track("z4_c_f1", tracker).width(110.dp).fillMaxHeight().background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape))
                Box(Modifier.track("z4_c_f2", tracker).width(105.dp).fillMaxHeight().background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape))
                Box(Modifier.weight(1f, fill = true).track("z4_c_w1", tracker).fillMaxHeight().background(RED))
                Box(Modifier.weight(3f, fill = true).track("z4_c_w3", tracker).fillMaxHeight().background(CYAN))
                Box(Modifier.track("z4_c_f3", tracker).width(64.dp).fillMaxHeight().background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape))
            }

            Row(
                Modifier.track("z4_d", tracker).fillMaxWidth().weight(1f)
                    .background(BG_SUBTLE).border(1.dp, BORDER_CARD, RectangleShape).padding(3.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Box(Modifier.track("z4_d_lead", tracker).width(42.dp).fillMaxHeight().background(TEAL))
                Spacer(Modifier.weight(1f))
                Column(
                    Modifier.weight(1.5f).track("z4_d_col", tracker).fillMaxHeight(),
                    verticalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    Box(Modifier.weight(1f).track("z4_d_top", tracker).fillMaxWidth().background(BLUE))
                    Box(Modifier.weight(2f).track("z4_d_bot", tracker).fillMaxWidth().background(EMERALD))
                }
            }
        }
    }
}

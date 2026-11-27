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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

@Composable
fun ParityAtlasScreen(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    viewportWidthDp: Int = 1760,
    viewportHeightDp: Int = 980,
    densityLabel: String = "1.0",
) {
    Column(
        Modifier
            .track("root", tracker)
            .fillMaxSize()
            .background(BG_CANVAS),
    ) {
        HeaderBar(fontFamily, tracker, viewportWidthDp, viewportHeightDp, densityLabel)

        Row(
            Modifier
                .track("main_body", tracker)
                .fillMaxWidth()
                .weight(1f)
                .padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(
                Modifier.weight(1.06f).track("col_1", tracker).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Zone1ModifierChainsAndConstraints(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.12f))
                Zone2BoxAlignmentAndZIndex(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.92f))
                Zone3ArrangementsAndOverflow(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.10f))
                Zone4WeightsFillAndMixed(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.86f))
            }

            Column(
                Modifier.weight(0.99f).track("col_2", tracker).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Zone5Intrinsics(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.04f))
                Zone6BaselinesAndText(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.02f))
                Zone7CustomLayoutAndBwc(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.00f))
                Zone8SubcomposeLayout(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.94f))
            }

            Column(
                Modifier.weight(1.00f).track("col_3", tracker).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Zone9RtlLtrDirectionality(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.18f))
                Zone10FlowLayouts(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.96f))
                Zone11OffsetsAndGraphicsLayer(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.94f))
                Zone12DrawModifiersAndShapes(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.92f))
            }

            Column(
                Modifier.weight(0.93f).track("col_4", tracker).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Zone13ScrollAndReverseLazyRow(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.88f))
                Zone14LazyColumnStickyHeader(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.06f))
                Zone15LazyGridsAndStaggered(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.12f))
                Zone16PagerAndDeepNesting(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.94f))
            }

            Column(
                Modifier.weight(1.02f).track("col_5", tracker).fillMaxHeight(),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Zone17LazyRowInLazyColumn(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.08f))
                Zone18CrossWrappedContainers(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.06f))
                Zone19LazyColInLazyRowAndAdaptiveGrid(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.96f))
                Zone20ModifierPermutationMatrix(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.90f))
            }
        }

        FooterBar(fontFamily, tracker)
    }
}

@Composable
private fun HeaderBar(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    w: Int,
    h: Int,
    densityLabel: String,
) {
    Row(
        Modifier
            .track("header_bar", tracker)
            .fillMaxWidth()
            .height(38.dp)
            .background(HEADER_BG)
            .padding(horizontal = 12.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(verticalAlignment = Alignment.Bottom) {
            BasicText(
                "LAYOUT PARITY ATLAS",
                Modifier.alignByBaseline().track("hdr_title", tracker),
                style = atlasTextStyle(fontFamily, 15f, Color(0xFFF8FAFC), bold = true),
            )
            BasicText(
                "Compose vs Cranpose",
                Modifier.alignByBaseline().track("hdr_sub", tracker).padding(start = 8.dp),
                style = atlasTextStyle(fontFamily, 10.5f, Color(0xFF94A3B8)),
            )
            BasicText(
                "20 ZONES · 80+ MECHANICS & WRAPPED COMBOS",
                Modifier.alignByBaseline().track("hdr_meta", tracker).padding(start = 8.dp),
                style = atlasTextStyle(fontFamily, 9f, Color(0xFF38BDF8), bold = true),
            )
        }
        Spacer(Modifier.weight(1f))
        Box(
            Modifier
                .track("hdr_pill", tracker)
                .background(Color(0xFF1E293B), RoundedCornerShape(10.dp))
                .border(1.dp, Color(0xFF334155), RoundedCornerShape(10.dp))
                .padding(horizontal = 10.dp, vertical = 3.dp),
        ) {
            BasicText(
                "${w}x${h} @ ${densityLabel}x",
                style = atlasTextStyle(fontFamily, 9.5f, Color(0xFFE2E8F0), bold = true),
            )
        }
    }
}

@Composable
private fun FooterBar(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(
        Modifier
            .track("footer_bar", tracker)
            .fillMaxWidth()
            .height(26.dp)
            .background(FOOTER_BG)
            .padding(horizontal = 12.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BasicText(
            "Constraints · Intrinsics · Baselines · Custom Layout · Subcompose · RTL/LTR · Flow · Offsets · GraphicsLayer · Draw · Scroll · StickyHeader · Grids · Pager",
            Modifier.track("ftr_left", tracker),
            style = atlasTextStyle(fontFamily, 8.5f, Color(0xFFCBD5E1)),
        )
        BasicText(
            "Deterministic Layout Parity Suite",
            Modifier.track("ftr_right", tracker),
            style = atlasTextStyle(fontFamily, 8.5f, Color(0xFF38BDF8), bold = true),
        )
    }
}

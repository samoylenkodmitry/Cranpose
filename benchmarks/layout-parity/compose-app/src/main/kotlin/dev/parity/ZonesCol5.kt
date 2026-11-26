@file:OptIn(ExperimentalFoundationApi::class, ExperimentalLayoutApi::class)

package dev.parity

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.staggeredgrid.LazyVerticalStaggeredGrid
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridCells
import androidx.compose.foundation.lazy.staggeredgrid.rememberLazyStaggeredGridState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PageSize
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

@Composable
internal fun Zone17LazyRowInLazyColumn(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val outerState = rememberLazyListState()
    ZoneCard("z17_card", "17. LazyRow in LazyColumn (With/Without height · Perms)", INDIGO, fontFamily, tracker, modifier) {
        LazyColumn(
            modifier = Modifier.track("z17_outer_lcol", tracker).fillMaxSize()
                .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
            state = outerState,
            contentPadding = PaddingValues(4.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            item(key = "lr_fixed_h") {
                val st = rememberLazyListState()
                LazyRow(
                    modifier = Modifier.track("z17_lr_fixed_h", tracker).fillMaxWidth().height(30.dp)
                        .background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape),
                    state = st,
                    contentPadding = PaddingValues(horizontal = 5.dp, vertical = 3.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(6, key = { it }) { i ->
                        LabeledChip("z17_f$i", "FH$i", 44.dp, 22.dp, ACCENTS[i % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                    }
                }
            }
            item(key = "lr_wrap_h") {
                val st = rememberLazyListState()
                val heights = listOf(16.dp, 28.dp, 20.dp, 24.dp, 18.dp, 26.dp)
                LazyRow(
                    modifier = Modifier.track("z17_lr_wrap_h", tracker).fillMaxWidth()
                        .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape),
                    state = st,
                    contentPadding = PaddingValues(horizontal = 5.dp, vertical = 3.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(heights.size, key = { it }) { i ->
                        LabeledChip("z17_w$i", "WH$i", 42.dp, heights[i], ACCENTS[(i + 2) % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                    }
                }
            }
            item(key = "lr_perm_a") {
                val st = rememberLazyListState()
                LazyRow(
                    modifier = Modifier.track("z17_lr_perm_a", tracker).padding(4.dp)
                        .background(EMERALD_SOFT).border(1.dp, EMERALD, RectangleShape).fillMaxWidth(),
                    state = st,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(6, key = { it }) { i ->
                        LabeledChip("z17_pa$i", "PA$i", 38.dp, 18.dp, EMERALD, 3.dp, 7.5f, fontFamily, tracker)
                    }
                }
            }
            item(key = "lr_perm_b") {
                val st = rememberLazyListState()
                val heights = listOf(16.dp, 40.dp, 22.dp, 18.dp, 26.dp)
                LazyRow(
                    modifier = Modifier.track("z17_lr_perm_b", tracker).fillMaxWidth()
                        .background(VIOLET_SOFT).padding(3.dp).border(1.dp, VIOLET, RectangleShape)
                        .sizeIn(minHeight = 24.dp, maxHeight = 32.dp),
                    state = st,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(heights.size, key = { it }) { i ->
                        LabeledChip("z17_pb$i", "PB$i", 40.dp, heights[i], VIOLET, 3.dp, 7.5f, fontFamily, tracker)
                    }
                }
            }
            item(key = "lr_rev_wrap") {
                val st = rememberLazyListState()
                LazyRow(
                    modifier = Modifier.track("z17_lr_rev_wrap", tracker).fillMaxWidth()
                        .background(ROSE_SOFT).border(1.dp, ROSE, RectangleShape),
                    state = st,
                    contentPadding = PaddingValues(horizontal = 4.dp, vertical = 3.dp),
                    reverseLayout = true,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(6, key = { it }) { i ->
                        LabeledChip("z17_rv$i", "RV$i", 40.dp, 20.dp, ROSE, 3.dp, 7.5f, fontFamily, tracker)
                    }
                }
            }
        }
    }
}

@Composable
internal fun Zone18CrossWrappedContainers(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z18_card", "18. Wrapped: Scroll/Flow/BWC/Intr/Pager in Lazy & VScroll", BLUE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Zone18MixedLazyColumn(fontFamily, tracker, Modifier.fillMaxWidth().weight(1.38f))
            Zone18LazyRowInVScroll(fontFamily, tracker, Modifier.fillMaxWidth().weight(0.62f))
        }
    }
}

@Composable
private fun Zone18MixedLazyColumn(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    val density = LocalDensity.current
    val outerLazy = rememberLazyListState()
    val hScrollInitPx = with(density) { 18.dp.roundToPx() }
    LazyColumn(
        modifier = modifier.track("z18_mix_lcol", tracker).background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
        state = outerLazy,
        contentPadding = PaddingValues(4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        item(key = "hs_in_lc") {
            val hs = rememberScrollState(hScrollInitPx)
            Row(
                Modifier.track("z18_hscroll_in_lc", tracker).fillMaxWidth()
                    .background(CYAN_SOFT).border(1.dp, CYAN, RectangleShape).horizontalScroll(hs).padding(3.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                repeat(6) { i ->
                    LabeledChip("z18_hs$i", "HS$i", 46.dp, 18.dp, ACCENTS[i % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
        item(key = "flow_in_lc") {
            FlowRow(
                Modifier.track("z18_flow_in_lc", tracker).fillMaxWidth()
                    .background(EMERALD_SOFT).border(1.dp, EMERALD, RectangleShape).padding(3.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                listOf(54.dp, 62.dp, 46.dp, 70.dp, 50.dp, 58.dp).forEachIndexed { i, w ->
                    LabeledChip("z18_fl$i", "F$i", w, 14.dp, EMERALD, 2.dp, 7f, fontFamily, tracker)
                }
            }
        }
        item(key = "bwc_in_lc") {
            BoxWithConstraints(
                Modifier.track("z18_bwc_in_lc", tracker).fillMaxWidth()
                    .background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape).padding(3.dp),
            ) {
                Box(
                    Modifier.track("z18_bwc_bar", tracker).width(maxWidth * 0.55f).height(14.dp).background(INDIGO),
                    contentAlignment = Alignment.CenterStart,
                ) { BasicText(" BWC-in-Lazy 55%", style = atlasTextStyle(fontFamily, 7.5f, Color.White, bold = true)) }
            }
        }
        item(key = "intr_in_lc") {
            Row(
                Modifier.track("z18_intr_in_lc", tracker).fillMaxWidth().height(IntrinsicSize.Min)
                    .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape).padding(3.dp),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                BasicText("Intrinsics inside lazy item wrap cleanly.", Modifier.weight(1f), style = atlasTextStyle(fontFamily, 8f, INK))
                Box(Modifier.track("z18_intr_div", tracker).width(3.dp).fillMaxHeight().background(RED))
                BasicText("OK", Modifier.align(Alignment.CenterVertically), style = atlasTextStyle(fontFamily, 8f, AMBER, bold = true))
            }
        }
        item(key = "pager_in_lc") {
            val ps = rememberPagerState(initialPage = 0) { 4 }
            HorizontalPager(
                state = ps,
                modifier = Modifier.track("z18_pager_in_lc", tracker).fillMaxWidth().height(26.dp)
                    .background(VIOLET_SOFT).border(1.dp, VIOLET, RectangleShape),
                contentPadding = PaddingValues(horizontal = 4.dp, vertical = 2.dp),
                pageSize = PageSize.Fixed(86.dp),
                pageSpacing = 4.dp,
            ) { p ->
                Box(
                    Modifier.track("z18_pg$p", tracker).fillMaxSize()
                        .background(ACCENTS[(p + 4) % ACCENTS.size], RoundedCornerShape(3.dp)),
                    contentAlignment = Alignment.Center,
                ) { BasicText("LPg$p", style = atlasTextStyle(fontFamily, 7.5f, Color.White, bold = true)) }
            }
        }
    }
}

@Composable
private fun Zone18LazyRowInVScroll(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    val outerVScroll = rememberScrollState(0)
    Column(
        modifier.track("z18_vscroll_outer", tracker)
            .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape)
            .verticalScroll(outerVScroll).padding(3.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        val lrState = rememberLazyListState()
        val hs = listOf(16.dp, 24.dp, 18.dp, 22.dp, 16.dp, 20.dp)
        LazyRow(
            modifier = Modifier.track("z18_lr_in_vscroll", tracker).fillMaxWidth()
                .background(CYAN_SOFT).border(1.dp, CYAN, RectangleShape),
            state = lrState,
            contentPadding = PaddingValues(3.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            items(hs.size, key = { it }) { i ->
                LabeledChip("z18_lvs$i", "VS$i", 44.dp, hs[i], CYAN, 3.dp, 7.5f, fontFamily, tracker)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            repeat(3) { i ->
                LabeledChip("z18_vs_tail$i", "Tail$i", 54.dp, 16.dp, SLATE, 2.dp, 7.5f, fontFamily, tracker)
            }
        }
    }
}

@Composable
internal fun Zone19LazyColInLazyRowAndAdaptiveGrid(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z19_card", "19. LazyCol/VScroll in LazyRow · AdaptiveGrid in BWC", EMERALD, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Zone19OuterLazyRow(fontFamily, tracker)
            Zone19AdaptiveAndStaggered(fontFamily, tracker, Modifier.fillMaxWidth().weight(1f))
        }
    }
}

@Composable
private fun Zone19OuterLazyRow(fontFamily: FontFamily, tracker: BoundsTracker) {
    val density = LocalDensity.current
    val outerRow = rememberLazyListState()
    val vScrollInitPx = with(density) { 12.dp.roundToPx() }
    LazyRow(
        modifier = Modifier.track("z19_outer_lrow", tracker).fillMaxWidth().height(82.dp)
            .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
        state = outerRow,
        contentPadding = PaddingValues(4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        item(key = "lc_fixed_w") {
            val st = rememberLazyListState()
            LazyColumn(
                modifier = Modifier.track("z19_lc_fixed_w", tracker).width(102.dp).fillMaxHeight()
                    .background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape),
                state = st,
                contentPadding = PaddingValues(3.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                items(5, key = { it }) { i ->
                    FullWidthChip("z19_lcf$i", "LC-F$i", 18.dp, BLUE, 2.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
        item(key = "lc_wrap_w") {
            val st = rememberLazyListState()
            val ws = listOf(52.dp, 86.dp, 68.dp, 60.dp)
            LazyColumn(
                modifier = Modifier.track("z19_lc_wrap_w", tracker).widthIn(min = 56.dp, max = 104.dp).fillMaxHeight()
                    .background(EMERALD_SOFT).border(1.dp, EMERALD, RectangleShape),
                state = st,
                contentPadding = PaddingValues(3.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                items(ws.size, key = { it }) { i ->
                    LabeledChip("z19_lcw$i", "W${ws[i].value.toInt()}", ws[i], 18.dp, EMERALD, 2.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
        item(key = "vs_in_lr") {
            val vs = rememberScrollState(vScrollInitPx)
            Column(
                Modifier.track("z19_vs_in_lr", tracker).width(88.dp).fillMaxHeight()
                    .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape).verticalScroll(vs).padding(3.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                repeat(5) { i ->
                    FullWidthChip("z19_vslr$i", "VS-R$i", 18.dp, AMBER, 2.dp, 7.5f, fontFamily, tracker)
                }
            }
        }
    }
}

@Composable
private fun Zone19AdaptiveAndStaggered(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        BoxWithConstraints(
            Modifier.weight(1.1f).track("z19_bwc_grid", tracker).fillMaxHeight()
                .background(VIOLET_SOFT).border(1.dp, VIOLET, RectangleShape),
        ) {
            val gs = rememberLazyGridState()
            LazyVerticalGrid(
                columns = GridCells.Adaptive(52.dp),
                modifier = Modifier.track("z19_adapt_grid", tracker).fillMaxSize(),
                state = gs,
                contentPadding = PaddingValues(3.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
                horizontalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                items(6, key = { it }) { i ->
                    FullWidthChip("z19_ag$i", "A$i", 18.dp, ACCENTS[i % ACCENTS.size], 2.dp, 7.5f, fontFamily, tracker)
                }
            }
        }

        val ss = rememberLazyStaggeredGridState()
        val sh = listOf(18.dp, 28.dp, 22.dp, 16.dp, 24.dp)
        LazyVerticalStaggeredGrid(
            columns = StaggeredGridCells.Fixed(2),
            modifier = Modifier.weight(0.9f).track("z19_row_stag", tracker).fillMaxHeight()
                .background(TEAL_SOFT).border(1.dp, TEAL, RectangleShape),
            state = ss,
            contentPadding = PaddingValues(3.dp),
            verticalItemSpacing = 3.dp,
            horizontalArrangement = Arrangement.spacedBy(3.dp),
        ) {
            items(sh.size, key = { it }) { i ->
                FullWidthChip("z19_rs$i", "S$i", sh[i], TEAL, 2.dp, 7.5f, fontFamily, tracker)
            }
        }
    }
}

@Composable
internal fun Zone20ModifierPermutationMatrix(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    ZoneCard("z20_card", "20. Modifier Permutations (Pad/Size/Clip/Offset/Layer/Scroll)", ROSE, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Zone20RowA(fontFamily, tracker)
            Zone20RowB(fontFamily, tracker)
            Zone20ScrollPermRow(fontFamily, tracker, Modifier.fillMaxWidth().weight(1f))
        }
    }
}

@Composable
private fun Zone20RowA(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.track("z20_p1", tracker).padding(5.dp).background(BLUE_SOFT)
                .border(1.dp, BLUE, RectangleShape).size(52.dp, 22.dp),
            contentAlignment = Alignment.Center,
        ) { BasicText("P->S", style = atlasTextStyle(fontFamily, 7.5f, BLUE, bold = true)) }

        Box(
            Modifier.track("z20_p2", tracker).size(52.dp, 22.dp).padding(5.dp)
                .background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape),
            contentAlignment = Alignment.Center,
        ) { BasicText("S->P", style = atlasTextStyle(fontFamily, 7.5f, BLUE, bold = true)) }

        Box(
            Modifier.track("z20_p3", tracker).background(AMBER_SOFT).padding(4.dp)
                .border(1.dp, AMBER, RectangleShape).padding(3.dp).background(EMERALD_SOFT).size(44.dp, 16.dp),
            contentAlignment = Alignment.Center,
        ) { BasicText("BgPBP", style = atlasTextStyle(fontFamily, 7f, INK, bold = true)) }

        Box(
            Modifier.track("z20_p4", tracker).requiredSize(52.dp, 22.dp).padding(4.dp)
                .background(VIOLET_SOFT).border(1.dp, VIOLET, RectangleShape),
            contentAlignment = Alignment.Center,
        ) { BasicText("Req->P", style = atlasTextStyle(fontFamily, 7f, VIOLET, bold = true)) }
    }
}

@Composable
private fun Zone20RowB(fontFamily: FontFamily, tracker: BoundsTracker) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier.size(54.dp, 24.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape)
                .clipToBounds().offset(12.dp, 6.dp).track("z20_p5", tracker).background(ROSE),
        )
        Box(
            Modifier.size(54.dp, 24.dp).background(BG_TRACK).border(1.dp, SLATE, RectangleShape)
                .offset(12.dp, 6.dp).track("z20_p6", tracker).clipToBounds().background(CYAN),
        )
        Box(
            Modifier.track("z20_p7", tracker).graphicsLayer { rotationZ = 10f }.padding(4.dp)
                .background(INDIGO_SOFT).border(1.dp, INDIGO, RectangleShape).size(48.dp, 18.dp),
            contentAlignment = Alignment.Center,
        ) { BasicText("GL->P", style = atlasTextStyle(fontFamily, 7.5f, INDIGO, bold = true)) }
        Box(
            Modifier.track("z20_p8", tracker).padding(4.dp).graphicsLayer { rotationZ = 10f }
                .background(TEAL_SOFT).border(1.dp, TEAL, RectangleShape).size(48.dp, 18.dp),
            contentAlignment = Alignment.Center,
        ) { BasicText("P->GL", style = atlasTextStyle(fontFamily, 7.5f, TEAL, bold = true)) }
    }
}

@Composable
private fun ScrollPermColumn(
    tagPrefix: String,
    labelPrefix: String,
    accent: Color,
    scrollBeforePadding: Boolean,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val density = LocalDensity.current
    val state = rememberScrollState(with(density) { 10.dp.roundToPx() })
    val base = modifier.track(tagPrefix, tracker).fillMaxHeight()
        .background(BG_SUBTLE).border(1.dp, accent, RectangleShape)
    val ordered = if (scrollBeforePadding) {
        base.verticalScroll(state).padding(5.dp)
    } else {
        base.padding(5.dp).verticalScroll(state)
    }
    Column(ordered, verticalArrangement = Arrangement.spacedBy(3.dp)) {
        repeat(6) { i ->
            FullWidthChip("${tagPrefix}_i$i", "$labelPrefix $i", 16.dp, accent, 2.dp, 7.5f, fontFamily, tracker)
        }
    }
}

@Composable
private fun Zone20ScrollPermRow(fontFamily: FontFamily, tracker: BoundsTracker, modifier: Modifier) {
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        ScrollPermColumn("z20_p9", "Scroll->Pad", BLUE, true, fontFamily, tracker, Modifier.weight(1f))
        ScrollPermColumn("z20_p10", "Pad->Scroll", ROSE, false, fontFamily, tracker, Modifier.weight(1f))
    }
}

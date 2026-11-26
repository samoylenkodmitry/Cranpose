@file:OptIn(ExperimentalFoundationApi::class)

package dev.parity

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.snapping.SnapPosition
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.staggeredgrid.LazyVerticalStaggeredGrid
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridCells
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridItemSpan
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

@Composable
internal fun Zone13ScrollAndReverseLazyRow(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val density = LocalDensity.current
    val vState = rememberScrollState(with(density) { 22.dp.roundToPx() })
    val hState = rememberScrollState(with(density) { 34.dp.roundToPx() })
    val lazyRowState = rememberLazyListState()

    ZoneCard("z13_card", "13. verticalScroll · horizontalScroll · LazyRow(reverse)", CYAN, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Column(
                Modifier.track("z13_vscroll", tracker).fillMaxWidth().height(50.dp)
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape)
                    .verticalScroll(vState).padding(4.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                repeat(5) { i ->
                    FullWidthChip(
                        "z13_vs_$i", "VScroll Row $i", 18.dp, ACCENTS[i % ACCENTS.size],
                        3.dp, 8f, fontFamily, tracker, Alignment.CenterStart, 6.dp,
                    )
                }
            }

            Row(
                Modifier.track("z13_hscroll", tracker).fillMaxWidth().height(26.dp)
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape)
                    .horizontalScroll(hState).padding(3.dp),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                repeat(7) { i ->
                    LabeledChip("z13_hs_$i", "HS $i", 56.dp, 18.dp, ACCENTS[(i + 1) % ACCENTS.size], 3.dp, 8f, fontFamily, tracker)
                }
            }

            LazyRow(
                modifier = Modifier.track("z13_lrow", tracker).fillMaxWidth().height(36.dp)
                    .background(BLUE_SOFT).border(1.dp, BLUE, RectangleShape),
                state = lazyRowState,
                contentPadding = PaddingValues(horizontal = 10.dp, vertical = 4.dp),
                reverseLayout = true,
                horizontalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                items(10, key = { it }) { i ->
                    LabeledChip("z13_lr_$i", "Rev $i", 46.dp, 26.dp, ACCENTS[i % ACCENTS.size], 4.dp, 8f, fontFamily, tracker)
                }
            }
        }
    }
}

@Composable
private fun StickyGroupRow(tag: String, label: String, bg: Color, stroke: Color, fontFamily: FontFamily, tracker: BoundsTracker) {
    Box(
        Modifier.track(tag, tracker).fillMaxWidth().height(24.dp)
            .background(bg, RoundedCornerShape(3.dp)).border(1.dp, stroke, RoundedCornerShape(3.dp))
            .padding(horizontal = 6.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        BasicText(label, style = atlasTextStyle(fontFamily, 8.5f, INK))
    }
}

@Composable
internal fun Zone14LazyColumnStickyHeader(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val density = LocalDensity.current
    val state = rememberLazyListState(2, with(density) { 10.dp.roundToPx() })

    ZoneCard("z14_card", "14. LazyColumn · Asymmetric Padding · Pinned stickyHeader", INDIGO, fontFamily, tracker, modifier) {
        LazyColumn(
            modifier = Modifier.track("z14_lcol", tracker).fillMaxSize()
                .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
            state = state,
            contentPadding = PaddingValues(start = 10.dp, top = 6.dp, end = 16.dp, bottom = 6.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            stickyHeader(key = "hdr_a") {
                FullWidthChip("z14_hdr_a", "SECTION A — PINNED HEADER", 22.dp, BLUE, 3.dp, 8.5f, fontFamily, tracker, Alignment.CenterStart, 6.dp)
            }
            items(4, key = { "a_$it" }) { i ->
                StickyGroupRow("z14_a$i", "Group A · Item $i", BLUE_SOFT, BLUE, fontFamily, tracker)
            }
            stickyHeader(key = "hdr_b") {
                FullWidthChip("z14_hdr_b", "SECTION B — APPROACHING", 22.dp, EMERALD, 3.dp, 8.5f, fontFamily, tracker, Alignment.CenterStart, 6.dp)
            }
            items(6, key = { "b_$it" }) { i ->
                StickyGroupRow("z14_b$i", "Group B · Item $i", EMERALD_SOFT, EMERALD, fontFamily, tracker)
            }
        }
    }
}

@Composable
internal fun Zone15LazyGridsAndStaggered(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val gridState = rememberLazyGridState()
    val staggeredState = rememberLazyStaggeredGridState()

    ZoneCard("z15_card", "15. LazyVerticalGrid (Spans/Wrap) · StaggeredGrid", AMBER, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            val gridHeights = listOf(18.dp, 18.dp, 28.dp, 22.dp, 20.dp, 20.dp, 18.dp, 18.dp)
            LazyVerticalGrid(
                columns = GridCells.Fixed(3),
                modifier = Modifier.track("z15_vgrid", tracker).fillMaxWidth().weight(1.05f)
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
                state = gridState,
                contentPadding = PaddingValues(4.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                items(
                    count = gridHeights.size,
                    key = { it },
                    span = { idx ->
                        when (idx) {
                            0 -> GridItemSpan(maxLineSpan)
                            4, 6, 7 -> GridItemSpan(2)
                            else -> GridItemSpan(1)
                        }
                    },
                ) { idx ->
                    val label = when (idx) {
                        0 -> "FullSpan (3)"
                        4, 6, 7 -> "Span2 #$idx"
                        else -> "G$idx"
                    }
                    FullWidthChip("z15_vg_$idx", label, gridHeights[idx], ACCENTS[idx % ACCENTS.size], 3.dp, 7.5f, fontFamily, tracker)
                }
            }

            val stagHeights = listOf(22.dp, 36.dp, 18.dp, 26.dp, 16.dp, 24.dp, 28.dp, 20.dp)
            LazyVerticalStaggeredGrid(
                columns = StaggeredGridCells.Fixed(3),
                modifier = Modifier.track("z15_sgrid", tracker).fillMaxWidth().weight(1f)
                    .background(AMBER_SOFT).border(1.dp, AMBER, RectangleShape),
                state = staggeredState,
                contentPadding = PaddingValues(4.dp),
                verticalItemSpacing = 4.dp,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                items(
                    count = stagHeights.size,
                    key = { it },
                    span = { idx ->
                        if (idx == 4) StaggeredGridItemSpan.FullLine else StaggeredGridItemSpan.SingleLane
                    },
                ) { idx ->
                    FullWidthChip(
                        "z15_sg_$idx",
                        if (idx == 4) "FullLine Barrier" else "M$idx",
                        stagHeights[idx],
                        if (idx == 4) INK else ACCENTS[(idx + 3) % ACCENTS.size],
                        3.dp, 7.5f, fontFamily, tracker,
                    )
                }
            }
        }
    }
}

@Composable
internal fun Zone16PagerAndDeepNesting(
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val pagerState = rememberPagerState(initialPage = 1) { 5 }

    ZoneCard("z16_card", "16. HorizontalPager (Fixed/Center) · 10-Level Nest", TEAL, fontFamily, tracker, modifier) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            HorizontalPager(
                state = pagerState,
                modifier = Modifier.track("z16_pager", tracker).fillMaxWidth().height(60.dp)
                    .background(BG_SUBTLE).border(1.dp, BORDER_STRONG, RectangleShape),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
                pageSize = PageSize.Fixed(128.dp),
                pageSpacing = 8.dp,
                snapPosition = SnapPosition.Center,
            ) { page ->
                Box(
                    Modifier.track("z16_pg_$page", tracker).fillMaxSize()
                        .background(ACCENTS[page % ACCENTS.size], RoundedCornerShape(6.dp))
                        .border(1.dp, Color.White, RoundedCornerShape(6.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    BasicText("Page $page", style = atlasTextStyle(fontFamily, 9.5f, Color.White, bold = true))
                }
            }

            DeepNestedChain(0, 10, fontFamily, tracker, Modifier.fillMaxWidth().weight(1f))
        }
    }
}

@Composable
private fun DeepNestedChain(
    level: Int,
    maxDepth: Int,
    fontFamily: FontFamily,
    tracker: BoundsTracker,
    modifier: Modifier,
) {
    val bg = if (level % 2 == 0) BLUE_SOFT else BG_CARD
    if (level >= maxDepth) {
        Box(
            modifier.track("z16_deep_core", tracker).fillMaxSize().background(BLUE),
            contentAlignment = Alignment.Center,
        ) {
            BasicText("Depth 10 Core", style = atlasTextStyle(fontFamily, 8f, Color.White, bold = true))
        }
        return
    }
    val mod = modifier.track("z16_deep_$level", tracker).fillMaxSize()
        .background(bg).border(1.dp, BORDER_CARD, RectangleShape)
        .padding(start = 3.dp, top = 2.dp, end = 2.dp, bottom = 2.dp)
    when (level % 3) {
        0 -> Box(mod) { DeepNestedChain(level + 1, maxDepth, fontFamily, tracker, Modifier) }
        1 -> Row(mod) { DeepNestedChain(level + 1, maxDepth, fontFamily, tracker, Modifier.weight(1f)) }
        else -> Column(mod) { DeepNestedChain(level + 1, maxDepth, fontFamily, tracker, Modifier.weight(1f)) }
    }
}

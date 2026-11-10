package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.IntState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.perfcompare.shared.Cards
import dev.perfcompare.shared.InTurn
import dev.perfcompare.shared.PATCHWORK_CARD_FRAMES
import dev.perfcompare.shared.PATCHWORK_CHART_POINTS
import dev.perfcompare.shared.PATCHWORK_CHART_STROKE
import dev.perfcompare.shared.PATCHWORK_DEPTH
import dev.perfcompare.shared.PATCHWORK_PILE_ALPHA
import dev.perfcompare.shared.PATCHWORK_PILE_OPAQUE_ARGB
import dev.perfcompare.shared.PATCH_COLUMNS
import dev.perfcompare.shared.PATCH_ROWS
import dev.perfcompare.shared.Patch
import dev.perfcompare.shared.PatchTiles
import dev.perfcompare.shared.CHAT_WIDTH
import dev.perfcompare.shared.WALL_CORNER
import dev.perfcompare.shared.WALL_GAP
import dev.perfcompare.shared.patchFloor
import dev.perfcompare.shared.patchTiles
import dev.perfcompare.shared.patchworkCardArgb
import dev.perfcompare.shared.patchworkCardTile
import dev.perfcompare.shared.patchworkCardTitle
import dev.perfcompare.shared.patchworkCards
import dev.perfcompare.shared.patchworkChartArgb
import dev.perfcompare.shared.patchworkChartValue
import dev.perfcompare.shared.patchworkDeepArgb
import dev.perfcompare.shared.patchworkDeepNumber
import dev.perfcompare.shared.patchworkDouble
import dev.perfcompare.shared.patchworkPairs
import dev.perfcompare.shared.patchworkParagraph
import dev.perfcompare.shared.patchworkParagraphWords
import dev.perfcompare.shared.patchworkPileGrid
import dev.perfcompare.shared.patchworkPileOpaque
import dev.perfcompare.shared.patchworkRecolorArgb
import dev.perfcompare.shared.patchworkReorderCount
import dev.perfcompare.shared.patchworkReorderItem
import dev.perfcompare.shared.patchworkReorderUnits
import dev.perfcompare.shared.patchworkReorderWidth
import dev.perfcompare.shared.patchworkScrollRows
import dev.perfcompare.shared.wallFillArgb
import dev.perfcompare.shared.wallLabelSize
import dev.perfcompare.shared.wallPadding
import dev.perfcompare.shared.wallTier
import kotlinx.coroutines.isActive

// The patchwork in Compose (`../../../../../../README.md`): nine patches of
// the still wall's tiles beside a chat. Two patches never change; in the
// others a frame changes a small part.
//
// In Compose's idiom a tile, a cell or a nest reads a state derived from the
// frame, which changes only when its own content does: a frame composes or
// draws again the part that changed and nothing else. The cards move in
// graphics layers, which compose and draw nothing again. The reorder patch's
// row composes again each frame with its items under their keys, and the
// pile patch in one call whose tiles skip but for the two whose fill changes.

/** The patchwork's tier and the frame it freezes on; 0 runs on. */
@Immutable
data class PatchworkLoad(val tier: Int, val freeze: Int)

/** One patch: its tiles and its size in points. */
@Immutable
private data class Grid(val tiles: PatchTiles, val patchWidth: Float, val patchHeight: Float) {
    /** A patch's own box, which cuts what lies past it. */
    val patch get() = Modifier.size(patchWidth.dp, patchHeight.dp).clipToBounds()
}

@Composable
fun ColumnScope.PatchworkScreen(load: PatchworkLoad) {
    val tier = remember(load.tier) { wallTier(load.tier) }
    val frame = remember { mutableIntStateOf(0) }
    Clock(frame)
    BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
        val patchWidth = patchFloor((maxWidth.value - CHAT_WIDTH) / PATCH_COLUMNS)
        val patchHeight = patchFloor(maxHeight.value / PATCH_ROWS)
        val grid = remember(tier, patchWidth, patchHeight) {
            Grid(patchTiles(tier, patchWidth, patchHeight), patchWidth, patchHeight)
        }
        Patches(frame, load.freeze, grid)
    }
}

@Composable
private fun Patches(frame: androidx.compose.runtime.MutableIntState, freeze: Int, grid: Grid) {
    val list = rememberLazyListState()
    val chat = rememberChat()
    val rowPx = with(LocalDensity.current) { (grid.tiles.height + WALL_GAP).dp.toPx() }
    LaunchedEffect(freeze, grid) {
        var index = 0
        while (isActive) {
            withFrameNanos { }
            index++
            frame.intValue = index
            // The list moves by one row each frame.
            list.dispatchRawDelta(rowPx)
            chat.advance()
            if (freeze in 1..index) {
                perfLog("PERF frozen frame=$index")
                break
            }
        }
    }
    Row {
        Column {
            for (row in 0 until PATCH_ROWS) {
                Row {
                    for (column in 0 until PATCH_COLUMNS) {
                        Show(Patch.entries[row * PATCH_COLUMNS + column], frame, list, grid)
                    }
                }
            }
        }
        Chat(frame, chat)
    }
}

@Composable
private fun Show(patch: Patch, frame: IntState, list: LazyListState, grid: Grid) {
    when (patch) {
        Patch.Still -> Box(grid.patch) { Wall(grid.tiles.columns, grid.tiles.rows, grid.tiles.width, grid.tiles.height) }
        Patch.Charts -> Charts(grid)
        Patch.Recolor -> Recolor(frame, grid)
        Patch.Cards -> CardStack(frame, grid)
        Patch.Reorder -> Reorder(frame, grid)
        Patch.Deep -> Deep(frame, grid)
        Patch.Scroll -> ScrollList(list, grid)
        Patch.Paragraph -> Paragraph(frame, grid)
        Patch.Pile -> Pile(frame, grid)
    }
}

// `columns` by `rows` cells, `WALL_GAP` apart and from the edges.
@Composable
private fun Cells(columns: Int, rows: Int, cell: @Composable (Int) -> Unit) {
    Column(Modifier.padding(WALL_GAP.dp), verticalArrangement = Arrangement.spacedBy(WALL_GAP.dp)) {
        for (row in 0 until rows) {
            Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP.dp)) {
                for (column in 0 until columns) {
                    cell(row * columns + column)
                }
            }
        }
    }
}

// Line charts that never change: each keeps its path in a draw cache, so no
// frame builds or records it again.
@Composable
private fun Charts(grid: Grid) {
    val (columns, rows) = patchworkPairs(grid.tiles.columns, grid.tiles.rows)
    val width = patchworkDouble(grid.tiles.width)
    val height = patchworkDouble(grid.tiles.height)
    Box(grid.patch) {
        Cells(columns, rows) { index ->
            Box(
                Modifier.size(width.dp, height.dp).drawWithCache {
                    val step = size.width / (PATCHWORK_CHART_POINTS - 1)
                    val line = Path()
                    for (point in 0 until PATCHWORK_CHART_POINTS) {
                        val y = size.height * (1f - patchworkChartValue(index, point))
                        if (point == 0) line.moveTo(0f, y) else line.lineTo(point * step, y)
                    }
                    val color = Color(patchworkChartArgb(index))
                    val stroke = Stroke(PATCHWORK_CHART_STROKE.dp.toPx())
                    onDrawBehind { drawPath(line, color, style = stroke) }
                },
            )
        }
    }
}

// Tiles whose fill changes in turn: each tile's draw block reads its own
// fill, derived from the frame, so a frame draws again the fills that changed.
@Composable
private fun Recolor(frame: IntState, grid: Grid) {
    Box(grid.patch) {
        Cells(grid.tiles.columns, grid.tiles.rows) { index -> RecolorTile(frame, index, grid.tiles) }
    }
}

@Composable
private fun RecolorTile(frame: IntState, index: Int, tiles: PatchTiles) {
    val fill by remember(index, tiles) {
        derivedStateOf { patchworkRecolorArgb(index, frame.intValue, tiles.inTurn) }
    }
    TileContent(
        index,
        tiles.height,
        Modifier
            .size(tiles.width.dp, tiles.height.dp)
            .drawBehind { drawRoundRect(Color(fill), cornerRadius = CornerRadius(WALL_CORNER.dp.toPx())) },
    )
}

// Cards that move every frame. The stack composes again when a move begins;
// within a move each card's graphics layer reads the frame for its place.
@Composable
private fun CardStack(frame: IntState, grid: Grid) {
    val cards = remember(grid) {
        patchworkCards(grid.tiles.columns, grid.tiles.rows, grid.tiles.height, grid.patchHeight)
    }
    val moves by remember { derivedStateOf { frame.intValue / PATCHWORK_CARD_FRAMES } }
    val begun = moves * PATCHWORK_CARD_FRAMES
    Box(grid.patch) {
        for (place in 0 until cards.places) {
            val card = cards.place(place, begun) ?: continue
            key(card.id) { Card(frame, cards, card.id, grid.tiles) }
        }
    }
}

@Composable
private fun Card(frame: IntState, cards: Cards, id: Int, tiles: PatchTiles) {
    Column(
        Modifier
            .padding(horizontal = WALL_GAP.dp)
            .graphicsLayer {
                // A card no frame shows waits above the patch.
                translationY = (cards.top(id, frame.intValue) ?: -cards.height).dp.toPx()
            },
        verticalArrangement = Arrangement.spacedBy(WALL_GAP.dp),
    ) {
        Box(
            Modifier
                .size(patchworkReorderWidth(cards.columns, tiles.width).dp, tiles.height.dp)
                .background(Color(patchworkCardArgb(id)), RoundedCornerShape(WALL_CORNER.dp))
                .padding(wallPadding(tiles.height).dp),
        ) {
            OneLine(patchworkCardTitle(id), wallLabelSize(tiles.height), true)
        }
        for (row in 0 until cards.rows) {
            Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP.dp)) {
                for (column in 0 until cards.columns) {
                    Tile(patchworkCardTile(id, row * cards.columns + column, cards.tiles), tiles.width, tiles.height)
                }
            }
        }
    }
}

// A wrapping row of items, two of which change places every frame: each item
// keeps its key, so a frame moves those two and composes none again.
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Reorder(frame: IntState, grid: Grid) {
    val count = patchworkReorderCount(grid.tiles.count)
    val turned = frame.intValue
    FlowRow(
        grid.patch.padding(WALL_GAP.dp),
        horizontalArrangement = Arrangement.spacedBy(WALL_GAP.dp),
        verticalArrangement = Arrangement.spacedBy(WALL_GAP.dp),
    ) {
        for (position in 0 until count) {
            val item = patchworkReorderItem(position, turned, count)
            key(item) {
                Tile(item, patchworkReorderWidth(patchworkReorderUnits(item), grid.tiles.width), grid.tiles.height)
            }
        }
    }
}

// Nests of boxes around digits that change in turn.
@Composable
private fun Deep(frame: IntState, grid: Grid) {
    val (columns, rows) = patchworkPairs(grid.tiles.columns, grid.tiles.rows)
    val turn = remember(columns, rows) { InTurn(columns, rows) }
    Box(grid.patch) {
        Cells(columns, rows) { index ->
            Box(Modifier.size(patchworkDouble(grid.tiles.width).dp, patchworkDouble(grid.tiles.height).dp)) {
                NestLevel(frame, index, 0, grid.tiles.height, turn)
            }
        }
    }
}

@Composable
private fun NestLevel(frame: IntState, index: Int, level: Int, height: Float, turn: InTurn) {
    Box(Modifier.fillMaxSize().background(Color(patchworkDeepArgb(index, level))).padding(wallPadding(height).dp)) {
        if (level + 1 < PATCHWORK_DEPTH) {
            NestLevel(frame, index, level + 1, height, turn)
        } else {
            NestValue(frame, index, height, turn)
        }
    }
}

// The digits inside a nest: the one composable of the nest that changes,
// when its own digits do.
@Composable
private fun NestValue(frame: IntState, index: Int, height: Float, turn: InTurn) {
    val number by remember(index, turn) { derivedStateOf { patchworkDeepNumber(index, frame.intValue, turn) } }
    OneLine(number.toString().padStart(3, '0'), wallLabelSize(height), true)
}

// A lazy list of rows of tiles, which the frame loop scrolls by one row
// every frame.
@Composable
private fun ScrollList(list: LazyListState, grid: Grid) {
    val tiles = grid.tiles
    LazyColumn(
        grid.patch,
        state = list,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(WALL_GAP.dp),
        verticalArrangement = Arrangement.spacedBy(WALL_GAP.dp),
        userScrollEnabled = false,
    ) {
        items(patchworkScrollRows(tiles.columns), key = { it }) { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(WALL_GAP.dp)) {
                for (column in 0 until tiles.columns) {
                    Tile(row * tiles.columns + column, tiles.width, tiles.height)
                }
            }
        }
    }
}

// One paragraph, one word of which changes every frame.
@Composable
private fun Paragraph(frame: IntState, grid: Grid) {
    Box(grid.patch.padding(WALL_GAP.dp)) { ParagraphText(frame, grid.tiles) }
}

@Composable
private fun ParagraphText(frame: IntState, tiles: PatchTiles) {
    BasicText(
        patchworkParagraph(frame.intValue, patchworkParagraphWords(tiles.count)),
        style = textStyle(wallLabelSize(tiles.height), INK, false),
    )
}

// Translucent tiles lying over one another. This call reads the frame; a
// tile composes again only when it turns opaque or translucent.
@Composable
private fun Pile(frame: IntState, grid: Grid) {
    val (columns, rows) = patchworkPileGrid(grid.tiles.columns, grid.tiles.rows)
    val opaque = patchworkPileOpaque(frame.intValue, columns * rows)
    Box(grid.patch) {
        for (index in 0 until columns * rows) {
            PileTile(index, index % columns, index / columns, grid.tiles, index == opaque)
        }
    }
}

@Composable
private fun PileTile(index: Int, column: Int, row: Int, tiles: PatchTiles, opaque: Boolean) {
    val fill = if (opaque) {
        Color(PATCHWORK_PILE_OPAQUE_ARGB)
    } else {
        Color(wallFillArgb(index)).copy(alpha = PATCHWORK_PILE_ALPHA / 255f)
    }
    TileContent(
        index,
        tiles.height,
        Modifier
            .offset(
                (WALL_GAP + column * (tiles.width + WALL_GAP)).dp,
                (WALL_GAP + row * (tiles.height + WALL_GAP)).dp,
            )
            .size(patchworkDouble(tiles.width).dp, patchworkDouble(tiles.height).dp)
            .background(fill, RoundedCornerShape(WALL_CORNER.dp)),
    )
}

/** One line of [text] that the box it lies in cuts. */
@Composable
internal fun OneLine(text: String, size: Float, bold: Boolean) {
    BasicText(text, style = textStyle(size, INK, bold), overflow = TextOverflow.Clip, softWrap = false, maxLines = 1)
}

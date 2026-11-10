package dev.perfcompare.shared

import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.sin

// The patchwork's data, as `perf-data/src/patchwork.rs` makes it bit for bit:
// nine patches of the still wall's tiles, three across and three down. Two
// never change; in the other seven a frame changes a small part and leaves
// the rest as it was.

/** The patches across and down the screen. */
const val PATCH_COLUMNS = 3
const val PATCH_ROWS = 3

/** The patches in the order the screen shows them, row by row. */
enum class Patch { Still, Charts, Recolor, Cards, Reorder, Deep, Scroll, Paragraph, Pile }

/** The step, in tiles or words, of everything that jumps. */
private const val JUMP = 37

/** One patch's tiles: [columns] by [rows] of them, [width] by [height] points each. */
data class PatchTiles(val columns: Int, val rows: Int, val width: Float, val height: Float) {
    val count get() = columns * rows
    val inTurn get() = InTurn(columns, rows)
}

/** The tiles of [tier] that fill a patch [patchWidth] by [patchHeight] points. */
fun patchTiles(tier: WallTier, patchWidth: Float, patchHeight: Float): PatchTiles {
    val (columns, rows) = wallGrid(tier, patchWidth, patchHeight)
    return PatchTiles(columns, rows, tier.tileWidth, tier.tileHeight)
}

/**
 * The items of a patch that change in turn: [columns] by [rows] of them. Each
 * frame changes the items of one row, the next row down the frame after.
 */
data class InTurn(val columns: Int, val rows: Int) {
    /** The changes of item [index] up to frame [frame]. */
    fun step(index: Int, frame: Int): Int = (index / columns.coerceAtLeast(1) + frame) / rows.coerceAtLeast(1)
}

/** The points of one chart of the charts patch, and the width of its line. */
const val PATCHWORK_CHART_POINTS = 512
const val PATCHWORK_CHART_STROKE = 1f

/** The height of chart [index] at [point], as a fraction of the chart. */
fun patchworkChartValue(index: Int, point: Int): Float {
    val at = point * 0.125f
    return 0.5f + 0.38f * sin(at * 0.32f + index * 0.77f) + 0.08f * sin(at * 1.7f + index)
}

/** The color of chart [index]'s line. */
fun patchworkChartArgb(index: Int): Int = PALETTE_ARGB[index % PALETTE_ARGB.size]

/** The fill of tile [index] of the recolor patch in frame [frame]. */
fun patchworkRecolorArgb(index: Int, frame: Int, turn: InTurn): Int =
    PALETTE_ARGB[(index + turn.step(index, frame)) % PALETTE_ARGB.size]

/** The frames one move of the cards patch takes. */
const val PATCHWORK_CARD_FRAMES = 32

/** A card of the cards patch in a frame: its number and its top in points from the patch's top. */
data class CardPlace(val id: Int, val top: Float)

/**
 * The cards patch: a card is [columns] tiles across and [rows] rows of tiles
 * under a header, [height] points tall, and [slots] cards rest in the patch.
 */
data class Cards(val columns: Int, val rows: Int, val height: Float, val slots: Int) {
    /** The places a frame may show a card at. */
    val places get() = slots + 1
    val tiles get() = columns * rows

    /**
     * The card at place [place] in frame [frame], the topmost first; none at
     * the last place while no card is on its way in. The moves alternate: a
     * new card slides in and the others move down by a card, then the first
     * and the third card move to each other's places and back.
     */
    fun place(place: Int, frame: Int): CardPlace? {
        val moves = frame / PATCHWORK_CARD_FRAMES
        val along = (frame % PATCHWORK_CARD_FRAMES).toFloat() / PATCHWORK_CARD_FRAMES
        val pitch = height + WALL_GAP
        val came = (moves + 1) / 2 + (moves + 1) % 2
        if (moves % 2 == 0) {
            val id = came + slots - 1 - place
            return if (id < 0) null else CardPlace(id, WALL_GAP + (place - 1f + along) * pitch)
        }
        if (place >= slots) return null
        val other = minOf(2, slots - 1)
        val reach = other * pitch * (1f - abs(2f * along - 1f))
        val moved = when (place) {
            0 -> reach
            other -> -reach
            else -> 0f
        }
        return CardPlace(came + slots - 1 - place, WALL_GAP + place * pitch + moved)
    }

    /** The top of card [id] in frame [frame], while the frame shows it. */
    fun top(id: Int, frame: Int): Float? {
        for (place in 0 until places) {
            val card = place(place, frame)
            if (card != null && card.id == id) return card.top
        }
        return null
    }
}

/** The cards of a patch [columns] by [rows] tiles of [tileHeight] points, [patchHeight] points tall. */
fun patchworkCards(columns: Int, rows: Int, tileHeight: Float, patchHeight: Float): Cards {
    val cardRows = (rows / 4).coerceAtLeast(1)
    val height = tileHeight + cardRows * (tileHeight + WALL_GAP)
    return Cards(columns, cardRows, height, ((patchHeight - WALL_GAP) / (height + WALL_GAP)).toInt().coerceAtLeast(1))
}

/** The header of card [id]. */
fun patchworkCardTitle(id: Int): String = "Group " + (id % 100_000).toString().padStart(5, '0')

/** The fill of card [id]'s header. */
fun patchworkCardArgb(id: Int): Int = GRADIENT_END_ARGB[id % GRADIENT_END_ARGB.size]

/** The wall tile at place [tile] of card [id], whose card holds [tiles] tiles. */
fun patchworkCardTile(id: Int, tile: Int, tiles: Int): Int = ((id.toLong() * tiles + tile) % 100_000).toInt()

/** The items of the reorder patch for a patch of [tiles] tiles. */
fun patchworkReorderCount(tiles: Int): Int = (tiles * 2 / 5).coerceAtLeast(1)

/** The first of the two neighbouring places whose items change places in frame [frame]. */
fun patchworkReorderSwap(frame: Int, count: Int): Int =
    ((frame.toLong() * JUMP) % (count - 1).coerceAtLeast(1)).toInt()

/** The item at place [position] of the reorder patch in frame [frame]. */
fun patchworkReorderItem(position: Int, frame: Int, count: Int): Int {
    val swap = patchworkReorderSwap(frame, count)
    return when {
        count < 2 -> position
        position == swap -> position + 1
        position == swap + 1 -> position - 1
        else -> position
    }
}

/** The width of item [item] of the reorder patch, in tiles: 1 to 3. */
fun patchworkReorderUnits(item: Int): Int = (((item.toUInt() * 2_654_435_761u) shr 22) % 3u).toInt() + 1

/** The width in points of an item [units] tiles wide. */
fun patchworkReorderWidth(units: Int, tileWidth: Float): Float = units * tileWidth + (units - 1) * WALL_GAP

/** The boxes around the digits of one nest of the deep patch. */
const val PATCHWORK_DEPTH = 12

/** The nests or the charts across and down a patch of [columns] by [rows] tiles. */
fun patchworkPairs(columns: Int, rows: Int): Pair<Int, Int> =
    Pair((columns / 2).coerceAtLeast(1), (rows / 2).coerceAtLeast(1))

/** The width or height of an item that covers 2 tiles of [tile] points and the gap between them. */
fun patchworkDouble(tile: Float): Float = 2f * tile + WALL_GAP

/** The fill of box [level] of nest [index], the outermost box being level 0. */
fun patchworkDeepArgb(index: Int, level: Int): Int = PALETTE_ARGB[(index + level) % PALETTE_ARGB.size]

/** The number inside nest [index] in frame [frame], under 1000. */
fun patchworkDeepNumber(index: Int, frame: Int, turn: InTurn): Int = (index * 7 + turn.step(index, frame)) % 1000

/** The rows of the scroll patch's list for rows of [columns] tiles. */
fun patchworkScrollRows(columns: Int): Int = 100_000 / columns.coerceAtLeast(1)

/** The first row the scroll patch shows in frame [frame]. */
fun patchworkScrollFirst(frame: Int, total: Int, visible: Int): Int =
    frame % ((total - visible).coerceAtLeast(0) + 1)

/** The words of the paragraph patch for a patch of [tiles] tiles. */
fun patchworkParagraphWords(tiles: Int): Int = tiles * 3

/** The paragraph in frame [frame]: [words] five-digit words, one of which shows the frame. */
fun patchworkParagraph(frame: Int, words: Int): String {
    val changed = ((frame.toLong() * JUMP) % words.coerceAtLeast(1)).toInt()
    val text = StringBuilder(words * 6)
    for (word in 0 until words) {
        if (word > 0) text.append(' ')
        text.append(wallLabel(if (word == changed) frame % 100_000 else word))
    }
    return text.toString()
}

/** The tiles across and down the pile patch for a patch of [columns] by [rows] tiles. */
fun patchworkPileGrid(columns: Int, rows: Int): Pair<Int, Int> =
    Pair((columns - 1).coerceAtLeast(1), (rows - 1).coerceAtLeast(1))

/** The opacity of the pile patch's tiles' fills, of 255, and the fill of its opaque tile. */
const val PATCHWORK_PILE_ALPHA = 0x99
const val PATCHWORK_PILE_OPAQUE_ARGB = 0xFF000000.toInt()

/** The tile of the pile patch whose fill is opaque in frame [frame]. */
fun patchworkPileOpaque(frame: Int, tiles: Int): Int = ((frame.toLong() * JUMP) % tiles.coerceAtLeast(1)).toInt()

/** The floor of [value], for patch sizes. */
fun patchFloor(value: Float): Float = floor(value)

package dev.perfcompare.shared

import kotlin.math.floor

// The still wall's data, as `perf-data/src/wall.rs` makes it bit for bit: a
// window of tiles that never change, under a clock line whose text changes
// every frame, with a cursor outline that rests on another tile every frame.

/** One load tier of the still wall: the size of its tiles, in whole points. */
data class WallTier(val tileWidth: Float, val tileHeight: Float)

/** The tiers, 1 to 20. */
val WALL_TIERS = listOf(
    WallTier(64f, 40f), WallTier(54f, 34f), WallTier(46f, 29f), WallTier(40f, 25f),
    WallTier(34f, 21f), WallTier(30f, 18f), WallTier(26f, 16f), WallTier(22f, 14f),
    WallTier(19f, 12f), WallTier(16f, 10f), WallTier(14f, 9f), WallTier(12f, 8f),
    WallTier(10f, 7f), WallTier(8f, 6f), WallTier(7f, 5f), WallTier(6f, 4f),
    WallTier(5f, 4f), WallTier(5f, 3f), WallTier(4f, 3f), WallTier(3f, 3f),
)

/** Tier 1 to 20; anything else is clamped into that range. */
fun wallTier(tier: Int): WallTier = WALL_TIERS[tier.coerceIn(1, WALL_TIERS.size) - 1]

/** The space between two tiles and around the wall, in points. */
const val WALL_GAP = 1f
/** The height of the clock line between the top bar and the wall, in points. */
const val WALL_CLOCK_HEIGHT = 24f
/** The clock line's text size, in points. */
const val WALL_CLOCK_TEXT = 14f
/** A tile's corner radius, in points. */
const val WALL_CORNER = 2f
/** The width of the cursor's outline, in points. */
const val WALL_CURSOR_WIDTH = 2f
/** The wall's background behind the tiles. */
const val WALL_BACKGROUND_ARGB = 0xFF0F1220.toInt()
/** The color of the tiles' text and of the cursor. */
const val WALL_INK_ARGB = 0xFFFFFFFF.toInt()

/** The tiles across and down that fill a wall [width] by [height] points with tiles of [tier]. */
fun wallGrid(tier: WallTier, width: Float, height: Float): Pair<Int, Int> {
    fun fit(space: Float, tile: Float) = floor((space - WALL_GAP) / (tile + WALL_GAP)).coerceAtLeast(1f).toInt()
    return Pair(fit(width, tier.tileWidth), fit(height, tier.tileHeight))
}

/** A tile's label: its index in five digits. */
fun wallLabel(index: Int): String = index.toString().padStart(5, '0')

/** A tile's value, which never changes: three digits that its index mixes. */
fun wallValue(index: Int): String {
    val mixed = (index.toUInt() * 2_654_435_761u) shr 22
    return (mixed % 1000u).toString().padStart(3, '0')
}

/** A tile's fill. */
fun wallFillArgb(index: Int): Int = PALETTE_ARGB[index % PALETTE_ARGB.size]

/** The color of a tile's dot. */
fun wallDotArgb(index: Int): Int = CHIP_BACKGROUND_ARGB[(index / PALETTE_ARGB.size) % CHIP_BACKGROUND_ARGB.size]

/** The tile the cursor rests on in frame [frame]: it jumps 37 tiles a frame. */
fun wallCursor(frame: Int, tiles: Int): Int = ((frame.toLong() * 37) % tiles.coerceAtLeast(1)).toInt()

/** The clock line in frame [frame]. */
fun wallClock(frame: Int): String = "frame " + frame.toString().padStart(6, '0')

/** The size of a tile's label and of its value for tiles [height] points tall. */
fun wallLabelSize(height: Float): Float = height * 0.30f
fun wallValueSize(height: Float): Float = height * 0.26f

/** A tile's padding for tiles [height] points tall. */
fun wallPadding(height: Float): Float = height * 0.06f

/** The diameter of a tile's dot for tiles [height] points tall. */
fun wallDotSize(height: Float): Float = height * 0.22f

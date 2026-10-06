package dev.perfcompare.shared

import kotlin.math.abs
import kotlin.math.sin

// Deterministic benchmark data the Android apps share. `cranpose-app/src/data.rs`
// implements the same generator bit for bit, so every app draws identical
// content. The Compose app marks these classes stable in
// `compose-app/app/compose_stability.conf`.

const val POST_COUNT = 5000
const val BAR_COUNT = 24

private val WORDS = arrayOf(
    "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do",
    "eiusmod", "tempor", "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "enim",
    "ad", "minim", "veniam", "quis", "nostrud", "exercitation", "ullamco", "laboris", "nisi",
    "aliquip", "ex", "ea", "commodo", "consequat", "duis", "aute", "irure", "in", "reprehenderit",
    "voluptate", "velit", "esse", "cillum", "fugiat", "nulla", "pariatur",
)
private val FIRST = arrayOf(
    "Ada", "Linus", "Grace", "Alan", "Barbara", "Dennis", "Ken", "Margaret", "Edsger", "Donald",
    "Frances", "John", "Radia", "Tim", "Guido", "Bjarne",
)
private val LAST = arrayOf(
    "Lovelace", "Torvalds", "Hopper", "Turing", "Liskov", "Ritchie", "Thompson", "Hamilton",
    "Dijkstra", "Knuth", "Allen", "McCarthy", "Perlman", "Berners", "Rossum", "Stroustrup",
)
private val TAGS = arrayOf(
    "#rust", "#kotlin", "#compose", "#android", "#gpu", "#layout", "#text", "#perf", "#wgpu",
    "#skia", "#ui", "#mobile",
)

/** Eight saturated colors as ARGB: avatars, chips, particles and tiles. */
val PALETTE_ARGB = intArrayOf(
    0xFFEF4444.toInt(), 0xFFF97316.toInt(), 0xFFEAB308.toInt(), 0xFF22C55E.toInt(),
    0xFF14B8A6.toInt(), 0xFF3B82F6.toInt(), 0xFF8B5CF6.toInt(), 0xFFEC4899.toInt(),
)
/** Light chip backgrounds matching [PALETTE_ARGB]. */
val CHIP_BACKGROUND_ARGB = intArrayOf(
    0xFFFEE2E2.toInt(), 0xFFFFEDD5.toInt(), 0xFFFEF9C3.toInt(), 0xFFDCFCE7.toInt(),
    0xFFCCFBF1.toInt(), 0xFFDBEAFE.toInt(), 0xFFEDE9FE.toInt(), 0xFFFCE7F3.toInt(),
)
/** Dark ends of the media gradients. */
val GRADIENT_END_ARGB = intArrayOf(
    0xFF7F1D1D.toInt(), 0xFF7C2D12.toInt(), 0xFF713F12.toInt(), 0xFF14532D.toInt(),
    0xFF134E4A.toInt(), 0xFF1E3A8A.toInt(), 0xFF4C1D95.toInt(), 0xFF831843.toInt(),
)


class Rng(seed: Int) {
    private var state: Int = (seed * 0x9E3779B9.toInt()) xor 0xA5A5A5A5.toInt()

    init {
        if (state == 0) state = 1
    }

    fun next(): Int {
        var x = state
        x = x xor (x shl 13)
        x = x xor (x ushr 17)
        x = x xor (x shl 5)
        state = x
        return x
    }

    fun below(bound: Int): Int = Integer.remainderUnsigned(next(), bound)

    fun unit(): Float = (next() ushr 8).toFloat() / 16_777_216f
}

class Post(
    val id: Int,
    val author: String,
    val handle: String,
    val initials: String,
    val color: Int,
    val title: String,
    val body: String,
    val bars: FloatArray,
    val tags: List<Pair<String, Int>>,
    val stats: List<String>,
) {
    // Posts are immutable, so identity decides equality.
    override fun equals(other: Any?): Boolean = other is Post && other.id == id
    override fun hashCode(): Int = id
}

private fun sentence(rng: Rng, min: Int, max: Int): String {
    val count = min + rng.below(max - min + 1)
    val text = (0 until count).joinToString(" ") { WORDS[rng.below(46)] }
    return text.replaceFirstChar { it.uppercaseChar() }
}

/** Formats a count the way social apps do: `734`, `12.4k`. */
fun compactCount(value: Int): String =
    if (value >= 1000) "${value / 1000}.${(value % 1000) / 100}k" else value.toString()

fun post(index: Int): Post {
    val rng = Rng(index + 1)
    val first = FIRST[rng.below(16)]
    val last = LAST[rng.below(16)]
    val minutes = rng.below(59) + 1
    val handle = "@${first.lowercase()}${rng.below(1000)} · ${minutes}m"
    val initials = "${first[0]}${last[0]}"
    val color = rng.below(8)
    val title = sentence(rng, 4, 8)
    val body = sentence(rng, 22, 38) + "."
    val bars = FloatArray(BAR_COUNT) { 0.15f + rng.unit() * 0.85f }
    val tags = List(3) {
        val tag = rng.below(TAGS.size)
        TAGS[tag] to tag % 8
    }
    val stats = List(4) { compactCount(rng.below(20_000)) }
    return Post(index, "$first $last", handle, initials, color, title, body, bars, tags, stats)
}

data class Comment(val author: String, val text: String, val color: Int)

/** The first [count] comments under post [id]. */
fun comments(id: Int, count: Int): List<Comment> = List(count) { index ->
    val rng = Rng(100_000 + id * 16 + index)
    val author = FIRST[rng.below(16)]
    val text = sentence(rng, 6, 14)
    Comment(author, text, index % 8)
}

fun posts(): List<Post> = List(POST_COUNT) { post(it) }

// ------------------------------------------------------------------ gauntlet
//
// Everything the gauntlet shows each frame is a function of the frame index,
// in integer arithmetic wherever a value is printed, so both apps show the
// same digits on the same frame.

/** What one load tier puts on screen; `../README.md` lists the tiers. */
data class GauntletTier(val columns: Int, val scale: Float, val tickers: Int, val depth: Int)

val GAUNTLET_TIERS = listOf(
    GauntletTier(columns = 1, scale = 1.0f, tickers = 8, depth = 6),
    GauntletTier(columns = 2, scale = 0.85f, tickers = 12, depth = 8),
    GauntletTier(columns = 2, scale = 0.7f, tickers = 16, depth = 10),
    GauntletTier(columns = 3, scale = 0.6f, tickers = 20, depth = 12),
    GauntletTier(columns = 3, scale = 0.5f, tickers = 28, depth = 14),
    GauntletTier(columns = 4, scale = 0.45f, tickers = 36, depth = 16),
    GauntletTier(columns = 4, scale = 0.4f, tickers = 44, depth = 20),
    GauntletTier(columns = 5, scale = 0.35f, tickers = 56, depth = 24),
    GauntletTier(columns = 6, scale = 0.3f, tickers = 72, depth = 28),
    GauntletTier(columns = 7, scale = 0.27f, tickers = 96, depth = 32),
    GauntletTier(columns = 8, scale = 0.25f, tickers = 120, depth = 40),
    GauntletTier(columns = 10, scale = 0.2f, tickers = 160, depth = 48),
    GauntletTier(columns = 12, scale = 0.18f, tickers = 200, depth = 56),
    GauntletTier(columns = 14, scale = 0.16f, tickers = 240, depth = 64),
    GauntletTier(columns = 16, scale = 0.14f, tickers = 300, depth = 72),
    GauntletTier(columns = 20, scale = 0.12f, tickers = 400, depth = 80),
)

/** Tier `1..16`; anything else is clamped into that range. */
fun gauntletTier(tier: Int): GauntletTier = GAUNTLET_TIERS[tier.coerceIn(1, GAUNTLET_TIERS.size) - 1]

/** Card rows between two deep clusters in the list. */
const val CARD_ROWS_PER_CLUSTER = 5
const val AVATAR_COUNT = 8
const val AVATAR_SIZE = 64
const val GAUNTLET_SPARK_POINTS = 48

class Ticker(val symbol: String, val baseCents: Int, val swingCents: Int, val step: Int, val phase: Int)

fun tickers(count: Int): List<Ticker> {
    val rng = Rng(31_337)
    return List(count) {
        val length = 3 + rng.below(2)
        val symbol = buildString { repeat(length) { append('A' + rng.below(26)) } }
        Ticker(
            symbol = symbol,
            baseCents = 1_000 + rng.below(99_000),
            swingCents = 50 + rng.below(950),
            step = 1 + rng.below(9),
            phase = rng.below(2_000),
        )
    }
}

/** A triangle wave over [period]: 0 at the ends, `period / 2` in the middle. */
private fun triangle(value: Int, period: Int): Int {
    val position = Math.floorMod(value, period)
    return period / 2 - abs(position - period / 2)
}

/** The quote's price on [frame], in cents. */
fun tickerCents(ticker: Ticker, frame: Int): Int {
    val wave = triangle(frame * ticker.step + ticker.phase, 2_000)
    return ticker.baseCents + ticker.swingCents * (wave - 500) / 500
}

/** `1234` → `12.34`. */
fun centsText(cents: Int): String {
    val sign = if (cents < 0) "-" else ""
    val value = abs(cents)
    val fraction = value % 100
    return "$sign${value / 100}.${if (fraction < 10) "0" else ""}$fraction"
}

/** The change from the base price as a signed percent: `+1.25%`. */
fun changeText(ticker: Ticker, cents: Int): String {
    val basisPoints = (cents - ticker.baseCents) * 10_000 / ticker.baseCents
    val sign = if (basisPoints < 0) "-" else "+"
    val value = abs(basisPoints)
    val fraction = value % 100
    return "$sign${value / 100}.${if (fraction < 10) "0" else ""}$fraction%"
}

/** A card's progress on [frame], in thousandths. */
fun progressPermille(card: Int, frame: Int): Int = (frame * 3 + card * 37) % 1_000

/** The tilt of a card's badge on [frame], in degrees: -5 to 5. */
fun badgeDegrees(card: Int, frame: Int): Float = triangle(frame * 2 + card * 30, 40) * 0.5f - 5f

/** The content's share of the screen width on [frame]: 0.92 to 1. */
fun widthFraction(frame: Int): Float = 0.92f + 0.08f * triangle(frame * 3, 200).toFloat() / 100f

/** The sparkline's height at [point], as a fraction of the chart, on [frame]. */
fun sparkValue(card: Int, point: Int, frame: Int): Float {
    val phase = (frame.toFloat() + card.toFloat() * 7f) * 0.11f
    return 0.5f + 0.38f * sin(point.toFloat() * 0.32f + phase) +
        0.08f * sin(point.toFloat() * 1.7f + card.toFloat())
}

/** The two ends of each avatar's gradient, as RGB bytes. */
private val AVATAR_COLORS = arrayOf(
    intArrayOf(0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D),
    intArrayOf(0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12),
    intArrayOf(0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12),
    intArrayOf(0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D),
    intArrayOf(0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A),
    intArrayOf(0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A),
    intArrayOf(0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95),
    intArrayOf(0xEC, 0x48, 0x99, 0x83, 0x18, 0x43),
)

/**
 * Avatar [index] as ARGB pixels: a radial gradient crossed by diagonal
 * stripes, so a misplaced or mis-scaled image is visible.
 */
fun avatarArgb(index: Int): IntArray {
    val colors = AVATAR_COLORS[index % AVATAR_COUNT]
    val size = AVATAR_SIZE
    val pixels = IntArray(size * size)
    for (y in 0 until size) {
        for (x in 0 until size) {
            val dx = x - size / 2
            val dy = y - size * 3 / 8
            val t = minOf(255, (dx * dx + dy * dy) * 255 / (40 * 40))
            val stripe = ((x + y + index * 3) / 6) % 2 == 0
            var argb = 0xFF shl 24
            for (channel in 0 until 3) {
                var value = (colors[channel] * (255 - t) + colors[channel + 3] * t) / 255
                if (stripe) value += (255 - value) / 6
                argb = argb or (value shl (16 - 8 * channel))
            }
            pixels[y * size + x] = argb
        }
    }
    return pixels
}

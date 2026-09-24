package dev.perfcompare.compose

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import kotlin.math.abs
import kotlin.math.floor

// Deterministic benchmark data. `cranpose-app/src/data.rs` implements the same
// generator bit for bit, so both apps draw identical content.

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

val PALETTE = arrayOf(
    Color(0xFFEF4444), Color(0xFFF97316), Color(0xFFEAB308), Color(0xFF22C55E),
    Color(0xFF14B8A6), Color(0xFF3B82F6), Color(0xFF8B5CF6), Color(0xFFEC4899),
)
val CHIP_BACKGROUND = arrayOf(
    Color(0xFFFEE2E2), Color(0xFFFFEDD5), Color(0xFFFEF9C3), Color(0xFFDCFCE7),
    Color(0xFFCCFBF1), Color(0xFFDBEAFE), Color(0xFFEDE9FE), Color(0xFFFCE7F3),
)
val GRADIENT_END = arrayOf(
    Color(0xFF7F1D1D), Color(0xFF7C2D12), Color(0xFF713F12), Color(0xFF14532D),
    Color(0xFF134E4A), Color(0xFF1E3A8A), Color(0xFF4C1D95), Color(0xFF831843),
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

@Immutable
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

@Immutable
data class Comment(val author: String, val text: String, val color: Int)

/** The first [count] comments under post [id]. */
fun comments(id: Int, count: Int): List<Comment> = List(count) { index ->
    val rng = Rng(100_000 + id * 16 + index)
    val author = FIRST[rng.below(16)]
    val text = sentence(rng, 6, 14)
    Comment(author, text, index % 8)
}

fun posts(): List<Post> = List(POST_COUNT) { post(it) }

@Immutable
data class Quote(val symbol: String, val base: Float, val speed: Float, val phase: Float)

fun quotes(count: Int): List<Quote> {
    val rng = Rng(7777)
    return List(count) {
        val length = 3 + rng.below(2)
        val symbol = buildString { repeat(length) { append('A' + rng.below(26)) } }
        val base = 10f + rng.unit() * 990f
        val speed = 0.5f + rng.unit() * 2.5f
        val phase = rng.unit() * 6.283f
        Quote(symbol, base, speed, phase)
    }
}

/**
 * Formats [value] with two decimals and a sign when asked, without the general
 * formatter, as a tuned app would on a per-frame path.
 */
fun fixed2(value: Float, signed: Boolean): String {
    val cents = Math.round(abs(value) * 100f)
    val sign = if (value < 0f) "-" else if (signed) "+" else ""
    val fraction = cents % 100
    return "$sign${cents / 100}.${if (fraction < 10) "0" else ""}$fraction"
}

/** Particles as parallel arrays: no per-particle objects on the draw path. */
class Particles(count: Int) {
    val x = FloatArray(count)
    val y = FloatArray(count)
    val vx = FloatArray(count)
    val vy = FloatArray(count)
    val size = FloatArray(count)
    val color = IntArray(count)
}

/** The first three quarters of [count] particles are circles, the rest rounded squares. */
fun isCircle(index: Int, count: Int): Boolean = index < count * 3 / 4

fun particles(count: Int): Particles {
    val rng = Rng(4242)
    val particles = Particles(count)
    for (index in 0 until count) {
        particles.x[index] = rng.unit()
        particles.y[index] = rng.unit()
        particles.vx[index] = (rng.unit() - 0.5f) * 0.4f
        particles.vy[index] = (rng.unit() - 0.5f) * 0.4f
        particles.size[index] =
            if (isCircle(index, count)) 3f + rng.unit() * 7f else 6f + rng.unit() * 14f
        particles.color[index] = rng.below(8)
    }
    return particles
}

/** Wraps [value] into `[0, 1)`. */
fun wrapUnit(value: Float): Float = value - floor(value)

package dev.perfcompare.compose

import dev.perfcompare.shared.*
import androidx.compose.runtime.Immutable
import kotlin.math.abs
import kotlin.math.floor

// Deterministic benchmark data. `cranpose-app/src/data.rs` implements the same
// generator bit for bit, so both apps draw identical content.

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

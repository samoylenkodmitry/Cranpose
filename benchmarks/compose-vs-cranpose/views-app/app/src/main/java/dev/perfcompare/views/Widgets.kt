package dev.perfcompare.views

import android.content.Context
import android.graphics.Canvas
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.util.TypedValue
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import dev.perfcompare.shared.GAUNTLET_SPARK_POINTS
import dev.perfcompare.shared.sparkValue
import kotlin.math.max
import kotlin.math.roundToInt

/** The device's Roboto faces, the same files the Compose and Cranpose apps load. */
class Fonts(val regular: Typeface, val bold: Typeface) {
    companion object {
        fun load() = Fonts(
            Typeface.createFromFile("/system/fonts/Roboto-Regular.ttf"),
            Typeface.createFromFile("/system/fonts/Roboto-Bold.ttf"),
        )
    }
}

fun Context.dp(value: Float): Float = value * resources.displayMetrics.density

fun Context.px(value: Float): Int = dp(value).roundToInt()

/**
 * Text whose lines are exactly 1.4 em apart, as in the other apps. Like
 * Compose's default line height style, the first line keeps no leading above
 * it and the last none below, so a block is (lines - 1) line heights plus the
 * font's own height. The platform rounds the line height to whole pixels;
 * the view takes the exact block height, rounded up as Compose rounds it.
 */
class GauntletText(context: Context) : TextView(context) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
        if (MeasureSpec.getMode(heightMeasureSpec) == MeasureSpec.EXACTLY) return
        val lines = layout?.lineCount?.coerceAtMost(maxLines) ?: return
        val metrics = paint.fontMetrics
        val block = (lines - 1) * textSize * 1.4f + metrics.descent - metrics.ascent
        setMeasuredDimension(measuredWidth, kotlin.math.ceil(block).toInt() + paddingTop + paddingBottom)
    }
}

/** Text of [sizeSp] in [color] with 1.4 em lines. */
fun GauntletText.styled(sizeSp: Float, color: Int, bold: Boolean, fonts: Fonts): GauntletText {
    setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
    setTextColor(color)
    typeface = if (bold) fonts.bold else fonts.regular
    includeFontPadding = false
    lineHeight = (textSize * 1.4f).roundToInt()
    return this
}

fun rounded(color: Int, radius: Float, strokeWidth: Int = 0, strokeColor: Int = 0) =
    GradientDrawable().apply {
        cornerRadius = radius
        setColor(color)
        if (strokeWidth > 0) setStroke(strokeWidth, strokeColor)
    }

/**
 * Measures its one child at [fraction] of its own width, read while
 * measuring: the frame lays everything out again without rebuilding views.
 */
class WidthFollower(context: Context, private val fraction: () -> Float) : ViewGroup(context) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        val height = MeasureSpec.getSize(heightMeasureSpec)
        val childWidth = (width * fraction()).roundToInt()
        getChildAt(0).measure(
            MeasureSpec.makeMeasureSpec(childWidth, MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(height, MeasureSpec.EXACTLY),
        )
        setMeasuredDimension(width, height)
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val child = getChildAt(0)
        child.layout(0, 0, child.measuredWidth, child.measuredHeight)
    }
}

/** Children left to right, wrapping, [horizontal] and [vertical] pixels apart. */
class FlowLayout(context: Context, private val horizontal: Int, private val vertical: Int) : ViewGroup(context) {
    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val maxWidth = MeasureSpec.getSize(widthMeasureSpec) - paddingLeft - paddingRight
        var x = 0
        var lineHeight = 0
        var height = 0
        var widest = 0
        val childSpec = MeasureSpec.makeMeasureSpec(maxWidth, MeasureSpec.AT_MOST)
        for (index in 0 until childCount) {
            val child = getChildAt(index)
            if (child.visibility == GONE) continue
            child.measure(childSpec, MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED))
            if (x > 0 && x + child.measuredWidth > maxWidth) {
                height += lineHeight + vertical
                x = 0
                lineHeight = 0
            }
            x += child.measuredWidth + horizontal
            widest = max(widest, x - horizontal)
            lineHeight = max(lineHeight, child.measuredHeight)
        }
        height += lineHeight
        val width = if (MeasureSpec.getMode(widthMeasureSpec) == MeasureSpec.EXACTLY) {
            MeasureSpec.getSize(widthMeasureSpec)
        } else {
            widest + paddingLeft + paddingRight
        }
        setMeasuredDimension(width, height + paddingTop + paddingBottom)
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val maxWidth = r - l - paddingLeft - paddingRight
        var x = 0
        var y = 0
        var lineHeight = 0
        for (index in 0 until childCount) {
            val child = getChildAt(index)
            if (child.visibility == GONE) continue
            if (x > 0 && x + child.measuredWidth > maxWidth) {
                y += lineHeight + vertical
                x = 0
                lineHeight = 0
            }
            val left = paddingLeft + x
            val top = paddingTop + y
            child.layout(left, top, left + child.measuredWidth, top + child.measuredHeight)
            x += child.measuredWidth + horizontal
            lineHeight = max(lineHeight, child.measuredHeight)
        }
    }
}

/** A rounded track filled to [fraction] in [fillColor]. */
class BarView(context: Context, private val radius: Float, trackColor: Int) : View(context) {
    private val track = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = trackColor }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rect = RectF()
    var fraction = 0f
        set(value) {
            if (field != value) {
                field = value
                invalidate()
            }
        }
    var fillColor: Int
        get() = fill.color
        set(value) {
            fill.color = value
        }

    override fun onDraw(canvas: Canvas) {
        rect.set(0f, 0f, width.toFloat(), height.toFloat())
        canvas.drawRoundRect(rect, radius, radius, track)
        rect.right = width * fraction
        canvas.drawRoundRect(rect, radius, radius, fill)
    }
}

/** A card's 48-point line over a fading fill, drawn from the frame. */
class SparklineView(context: Context, strokeWidth: Float, private val frame: () -> Int) : View(context) {
    private val area = Path()
    private val line = Path()
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        this.strokeWidth = strokeWidth
    }
    private var card = 0
    private var color = 0
    private var shaderHeight = -1

    fun bind(card: Int, color: Int) {
        this.card = card
        if (this.color != color) {
            this.color = color
            stroke.color = color
            shaderHeight = -1
        }
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        val w = width.toFloat()
        val h = height.toFloat()
        if (shaderHeight != height) {
            shaderHeight = height
            fill.shader = LinearGradient(
                0f, 0f, 0f, h,
                (color and 0x00FFFFFF) or (64 shl 24), color and 0x00FFFFFF,
                Shader.TileMode.CLAMP,
            )
        }
        val step = w / (GAUNTLET_SPARK_POINTS - 1)
        val k = frame()
        area.rewind()
        line.rewind()
        area.moveTo(0f, h)
        for (index in 0 until GAUNTLET_SPARK_POINTS) {
            val x = index * step
            val y = h * (1f - sparkValue(card, index, k))
            area.lineTo(x, y)
            if (index == 0) line.moveTo(x, y) else line.lineTo(x, y)
        }
        area.lineTo(w, h)
        area.close()
        canvas.drawPath(area, fill)
        canvas.drawPath(line, stroke)
    }
}

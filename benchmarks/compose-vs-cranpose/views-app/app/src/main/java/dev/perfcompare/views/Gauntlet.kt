package dev.perfcompare.views

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Outline
import android.graphics.Rect
import android.graphics.drawable.GradientDrawable
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextUtils
import android.text.style.ForegroundColorSpan
import android.text.style.TypefaceSpan
import android.util.Log
import android.view.Choreographer
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewOutlineProvider
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import dev.perfcompare.shared.AVATAR_COUNT
import dev.perfcompare.shared.AVATAR_SIZE
import dev.perfcompare.shared.CARD_ROWS_PER_CLUSTER
import dev.perfcompare.shared.CHIP_BACKGROUND_ARGB
import dev.perfcompare.shared.GRADIENT_END_ARGB
import dev.perfcompare.shared.GauntletTier
import dev.perfcompare.shared.LAYER_COLUMNS
import dev.perfcompare.shared.PALETTE_ARGB
import dev.perfcompare.shared.Post
import dev.perfcompare.shared.Ticker
import dev.perfcompare.shared.avatarArgb
import dev.perfcompare.shared.badgeDegrees
import dev.perfcompare.shared.centsText
import dev.perfcompare.shared.changeText
import dev.perfcompare.shared.layerCellColor
import dev.perfcompare.shared.layerDegrees
import dev.perfcompare.shared.layerX
import dev.perfcompare.shared.layerY
import dev.perfcompare.shared.posts
import dev.perfcompare.shared.progressPermille
import dev.perfcompare.shared.tickerCents
import dev.perfcompare.shared.tickers
import dev.perfcompare.shared.widthFraction

// The gauntlet in Android Views, element for element the screen the Compose
// and Cranpose apps draw, in the Views toolkit's own idiom: a RecyclerView
// grid, HWUI elevation shadows, and one Choreographer callback that updates
// only what changes each frame. The benchmark's README describes the screen.

private const val INK = 0xFF111827.toInt()
private const val BODY = 0xFF374151.toInt()
private const val MUTED = 0xFF6B7280.toInt()
private const val HAIRLINE = 0xFFE5E7EB.toInt()
private const val PANEL = 0xFFE2E8F0.toInt()
private const val UP = 0xFF16A34A.toInt()
private const val DOWN = 0xFFDC2626.toInt()
private val LEVEL_BACKGROUND = intArrayOf(0xFFF1F5F9.toInt(), 0xFFCBD5E1.toInt())
private const val LAYER_BACKGROUND = 0xC01E293B.toInt()
private const val NESTED_BACKGROUND = 0xE6FFFFFF.toInt()

/** Blocks of five card rows and a cluster: no measurement window reaches the end. */
private const val BLOCKS = 200_000
private const val SCROLL_PER_FRAME_DP = 3f
private const val CARD = 0
private const val CLUSTER = 1
private val FOOTER_LABELS = arrayOf("likes", "replies", "shares")

/** Everything the screen shares, and the frame it draws. */
class GauntletState(
    val context: Context,
    val tier: GauntletTier,
    val fonts: Fonts,
) {
    var frame = 0
    val s = tier.scale
    val posts: List<Post> = posts()
    val tickers: List<Ticker> = tickers(tier.tickers)
    val avatars: List<Bitmap> = List(AVATAR_COUNT) {
        Bitmap.createBitmap(avatarArgb(it), AVATAR_SIZE, AVATAR_SIZE, Bitmap.Config.ARGB_8888)
    }

    fun px(value: Float) = context.px(value * s)
    fun text(sizeSp: Float, color: Int, bold: Boolean) =
        GauntletText(context).styled(sizeSp * s, color, bold, fonts)
}

/** Builds the screen under [parent] and drives it from the frame clock. */
class GauntletScreen(private val state: GauntletState, private val freeze: Int) : Choreographer.FrameCallback {
    private val context = state.context
    private val tiles = mutableListOf<TickerTile>()
    private val layers = List(state.tier.layers) { StackedLayer(state, it) }
    private val recycler = RecyclerView(context)
    private val follower: WidthFollower
    private val scrollPx = context.dp(SCROLL_PER_FRAME_DP).let { kotlin.math.round(it).toInt() }
    // What the list has scrolled: a scroll asked for before it has laid out
    // is dropped, so each frame asks for what is still owed, as Compose's and
    // Cranpose's lists keep a pending scroll until they next measure.
    private var scrolled = 0
    val root: View

    init {
        val column = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        val panel = FlowLayout(context, state.px(4f), state.px(4f)).apply {
            setBackgroundColor(PANEL)
            val pad = state.px(6f)
            setPadding(pad, pad, pad, pad)
        }
        state.tickers.forEachIndexed { index, ticker ->
            val tile = TickerTile(state, ticker, index)
            tiles += tile
            panel.addView(tile.view)
        }
        column.addView(panel, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        val pad = state.px(8f)
        recycler.apply {
            setPadding(pad, pad, pad, pad)
            clipToPadding = false
            setHasFixedSize(true)
            itemAnimator = null
            // Rows of cards in a linear list: one anchor per row, so a row whose
            // cards change height as the width moves never re-anchors the list.
            layoutManager = LinearLayoutManager(context)
            addItemDecoration(RowSpacing(state.px(8f)))
            adapter = GauntletAdapter(state)
            addOnScrollListener(object : RecyclerView.OnScrollListener() {
                override fun onScrolled(recyclerView: RecyclerView, dx: Int, dy: Int) {
                    scrolled += dy
                }
            })
        }
        val stack = FrameLayout(context).apply {
            addView(recycler, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            for (layer in layers) {
                addView(layer.view, FrameLayout.LayoutParams(context.px(220f), ViewGroup.LayoutParams.WRAP_CONTENT))
            }
        }
        column.addView(stack, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        follower = WidthFollower(context) { widthFraction(state.frame) }.apply { addView(column) }
        root = follower
    }

    fun start() = Choreographer.getInstance().postFrameCallback(this)

    override fun doFrame(frameTimeNanos: Long) {
        val frame = ++state.frame
        for (tile in tiles) tile.onFrame(frame)
        for (layer in layers) layer.onFrame(frame)
        for (index in 0 until recycler.childCount) {
            (recycler.getChildViewHolder(recycler.getChildAt(index)) as? CardRowHolder)?.onFrame(frame)
        }
        recycler.scrollBy(0, frame * scrollPx - scrolled)
        follower.requestLayout()
        if (freeze in 1..frame) {
            Log.i(TAG, "PERF frozen frame=$frame")
            return
        }
        Choreographer.getInstance().postFrameCallback(this)
    }
}

/** The space between list rows. */
private class RowSpacing(private val spacing: Int) : RecyclerView.ItemDecoration() {
    override fun getItemOffsets(out: Rect, view: View, parent: RecyclerView, state: RecyclerView.State) {
        out.bottom = spacing
    }
}

private class TickerTile(private val state: GauntletState, private val ticker: Ticker, index: Int) {
    private val price = state.text(10f, BODY, false)
    private val change = state.text(10f, UP, false)
    private val bar = BarView(state.context, state.context.dp(2f * state.s), HAIRLINE).apply {
        fillColor = PALETTE_ARGB[index % PALETTE_ARGB.size]
    }
    val view = LinearLayout(state.context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        background = rounded(0xFFFFFFFF.toInt(), state.context.dp(6f * state.s))
        setPadding(state.px(6f), state.px(3f), state.px(6f), state.px(3f))
        val gap = state.px(4f)
        fun spaced(width: Int = ViewGroup.LayoutParams.WRAP_CONTENT, height: Int = ViewGroup.LayoutParams.WRAP_CONTENT, last: Boolean = false) =
            LinearLayout.LayoutParams(width, height).apply { if (!last) marginEnd = gap }
        addView(state.text(10f, INK, true).apply { text = ticker.symbol }, spaced())
        addView(price, spaced())
        addView(change, spaced())
        addView(bar, spaced(state.px(20f), state.px(4f), last = true))
    }

    fun onFrame(frame: Int) {
        val cents = tickerCents(ticker, frame)
        price.text = centsText(cents)
        change.text = changeText(ticker, cents)
        change.setTextColor(if (cents >= ticker.baseCents) UP else DOWN)
        bar.fraction = ((cents - ticker.baseCents + ticker.swingCents).toFloat() / (2 * ticker.swingCents)).coerceIn(0f, 1f)
    }
}

/**
 * A translucent panel stacked over the list. Its views never change: each
 * frame sets only its render node's translation and rotation, and its nested
 * card's rotation the other way.
 */
private class StackedLayer(state: GauntletState, private val layer: Int) {
    private val context = state.context
    private val fonts = state.fonts
    private val nested = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(NESTED_BACKGROUND, context.dp(8f))
        val pad = context.px(8f)
        setPadding(pad, pad, pad, pad)
        addView(text(11f, INK, true, "Nested in layer ${layer + 1}"))
        addView(text(11f, BODY, false, "Tilts against its panel"), below(2f))
    }
    val view = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(LAYER_BACKGROUND, context.dp(12f))
        val pad = context.px(10f)
        setPadding(pad, pad, pad, pad)
        addView(text(13f, 0xFFFFFFFF.toInt(), true, "Layer ${layer + 1}"))
        for (row in 0 until state.tier.layerRows) addView(cells(row), below(if (row == 0) 8f else 3f))
        addView(nested, below(8f).apply { width = ViewGroup.LayoutParams.MATCH_PARENT })
    }

    init {
        onFrame(0)
    }

    private fun text(sizeSp: Float, color: Int, bold: Boolean, value: String) =
        GauntletText(context).styled(sizeSp, color, bold, fonts).apply { text = value }

    private fun below(gapDp: Float) =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
            topMargin = context.px(gapDp)
        }

    private fun cells(row: Int) = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        for (column in 0 until LAYER_COLUMNS) {
            val cell = row * LAYER_COLUMNS + column
            addView(
                text(9f, 0xFFFFFFFF.toInt(), false, "${cell + 1}").apply {
                    gravity = Gravity.CENTER
                    background = rounded(PALETTE_ARGB[layerCellColor(layer, cell)], context.dp(4f))
                },
                LinearLayout.LayoutParams(context.px(22f), context.px(22f)).apply {
                    if (column > 0) marginStart = context.px(3f)
                },
            )
        }
    }

    fun onFrame(frame: Int) {
        val degrees = layerDegrees(layer, frame)
        view.translationX = context.dp(layerX(layer, frame))
        view.translationY = context.dp(layerY(layer, frame))
        view.rotation = degrees
        nested.rotation = -degrees
    }
}

/** Blocks of five card rows and a deep cluster. */
private class GauntletAdapter(private val state: GauntletState) :
    RecyclerView.Adapter<RecyclerView.ViewHolder>() {
    init {
        setHasStableIds(true)
    }

    override fun getItemCount() = BLOCKS * (CARD_ROWS_PER_CLUSTER + 1)
    override fun getItemId(position: Int) = position.toLong()
    override fun getItemViewType(position: Int) =
        if (position % (CARD_ROWS_PER_CLUSTER + 1) == CARD_ROWS_PER_CLUSTER) CLUSTER else CARD

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): RecyclerView.ViewHolder =
        if (viewType == CLUSTER) ClusterHolder(state) else CardRowHolder(state)

    override fun onBindViewHolder(holder: RecyclerView.ViewHolder, position: Int) {
        val block = position / (CARD_ROWS_PER_CLUSTER + 1)
        val within = position % (CARD_ROWS_PER_CLUSTER + 1)
        when (holder) {
            is ClusterHolder -> holder.bind(block)
            is CardRowHolder -> holder.bind((block * CARD_ROWS_PER_CLUSTER + within) * state.tier.columns)
        }
    }
}

/** A row of cards, as wide as the list and as tall as its tallest card. */
private class CardRowHolder(private val state: GauntletState) :
    RecyclerView.ViewHolder(LinearLayout(state.context)) {
    private val cards = List(state.tier.columns) { CardView(state) }

    init {
        val row = itemView as LinearLayout
        row.orientation = LinearLayout.HORIZONTAL
        row.clipChildren = false
        row.layoutParams = RecyclerView.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        cards.forEachIndexed { index, card ->
            row.addView(card.view, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply {
                if (index > 0) marginStart = state.px(8f)
            })
        }
    }

    fun bind(first: Int) = cards.forEachIndexed { index, card -> card.bind(first + index) }

    fun onFrame(frame: Int) = cards.forEach { it.onFrame(frame) }
}

private class CardView(private val state: GauntletState) {
    val view = FrameLayout(state.context)
    private val context = state.context
    private val avatar = ImageView(context).apply {
        scaleType = ImageView.ScaleType.CENTER_CROP
        outlineProvider = object : ViewOutlineProvider() {
            override fun getOutline(view: View, outline: Outline) = outline.setOval(0, 0, view.width, view.height)
        }
        clipToOutline = true
    }
    private val title = state.text(13f, INK, true).apply {
        maxLines = 2
        ellipsize = TextUtils.TruncateAt.END
    }
    private val subtitle = state.text(11f, MUTED, false).apply {
        maxLines = 1
        ellipsize = TextUtils.TruncateAt.END
    }
    private val body = state.text(12f, BODY, false).apply {
        maxLines = 4
        ellipsize = TextUtils.TruncateAt.END
    }
    private val progress = BarView(context, context.dp(3f * state.s), HAIRLINE)
    private val percent = state.text(10f, MUTED, false)
    private val sparkline = SparklineView(context, context.dp(1.5f * state.s)) { state.frame }
    private val chips = FlowLayout(context, state.px(4f), state.px(4f))
    private val stats = Array(3) { state.text(12f, INK, true) }
    private val badge = state.text(9f, 0xFFFFFFFF.toInt(), true).apply {
        text = "HOT"
        background = rounded(PALETTE_ARGB[0], context.dp(8f * state.s))
        setPadding(state.px(6f), state.px(2f), state.px(6f), state.px(2f))
        alpha = 0.9f
        // Above the elevated card body, as drawn after it, without a shadow.
        outlineProvider = null
        translationZ = context.dp(3f * state.s)
    }
    private var card = 0
    private var shownPercent = -1

    init {
        val frame = view
        val radius = context.dp(12f * state.s)
        val content = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = rounded(0xFFFFFFFF.toInt(), radius, context.px(1f), HAIRLINE)
            elevation = context.dp(3f * state.s)
            outlineProvider = ViewOutlineProvider.BACKGROUND
            clipToOutline = true
            val pad = state.px(10f)
            setPadding(pad, pad, pad, pad)
        }
        val gap = state.px(6f)
        fun block(view: View, height: Int = ViewGroup.LayoutParams.WRAP_CONTENT, first: Boolean = false) =
            content.addView(view, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, height).apply {
                if (!first) topMargin = gap
            })
        val header = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            val size = state.px(32f)
            addView(avatar, LinearLayout.LayoutParams(size, size).apply { marginEnd = state.px(8f) })
            addView(LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                addView(title)
                addView(subtitle)
            }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        }
        block(header, first = true)
        block(body)
        block(LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(progress, LinearLayout.LayoutParams(0, state.px(6f), 1f).apply { marginEnd = state.px(6f) })
            addView(percent)
        })
        block(sparkline, state.px(36f))
        block(chips)
        // Dividers between the counters, as tall as the row: the toolkit's own
        // middle dividers.
        block(LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            dividerDrawable = rounded(HAIRLINE, 0f).apply { setSize(context.px(1f), 0) }
            showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
            FOOTER_LABELS.forEachIndexed { index, label ->
                addView(LinearLayout(context).apply {
                    orientation = LinearLayout.VERTICAL
                    gravity = Gravity.CENTER_HORIZONTAL
                    // Each text as wide as itself, centered: a vertical
                    // layout otherwise stretches it and it draws from the start.
                    val wrap = { LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT) }
                    addView(stats[index], wrap())
                    addView(state.text(9f, MUTED, false).apply { text = label }, wrap())
                }, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            }
        })
        frame.addView(content, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        frame.addView(badge, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP or Gravity.END).apply {
            val margin = state.px(6f)
            setMargins(margin, margin, margin, margin)
        })
        frame.clipChildren = false
    }

    fun bind(card: Int) {
        this.card = card
        val post = state.posts[card % state.posts.size]
        avatar.setImageBitmap(state.avatars[card % AVATAR_COUNT])
        title.text = post.title
        subtitle.text = subtitle(post)
        body.text = post.body
        progress.fillColor = PALETTE_ARGB[card % PALETTE_ARGB.size]
        sparkline.bind(card, PALETTE_ARGB[post.color])
        bindChips(post)
        for (index in 0 until 3) stats[index].text = post.stats[index]
        badge.visibility = if (card % 5 == 0) View.VISIBLE else View.GONE
        shownPercent = -1
        onFrame(state.frame)
    }

    private fun bindChips(post: Post) {
        while (chips.childCount < post.tags.size) {
            chips.addView(state.text(10f, INK, false).apply {
                setPadding(state.px(8f), state.px(3f), state.px(8f), state.px(3f))
                background = rounded(0, context.dp(10f * state.s))
            })
        }
        post.tags.forEachIndexed { index, (tag, color) ->
            (chips.getChildAt(index) as TextView).apply {
                text = tag
                setTextColor(GRADIENT_END_ARGB[color])
                (background as GradientDrawable).setColor(CHIP_BACKGROUND_ARGB[color])
            }
        }
    }

    private fun subtitle(post: Post): CharSequence {
        val (tag, color) = post.tags[0]
        return SpannableStringBuilder().apply {
            append("by ")
            val author = length
            append(post.author)
            setSpan(TypefaceSpan(state.fonts.bold), author, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
            append(" · ")
            val start = length
            append(tag)
            setSpan(ForegroundColorSpan(GRADIENT_END_ARGB[color]), start, length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
    }

    /** What the frame changes: the progress and its percent, the sparkline, the badge tilt. */
    fun onFrame(frame: Int) {
        val permille = progressPermille(card, frame)
        progress.fraction = permille / 1000f
        val shown = permille / 10
        if (shown != shownPercent) {
            shownPercent = shown
            percent.text = "$shown%"
        }
        sparkline.invalidate()
        if (badge.visibility == View.VISIBLE) badge.rotation = badgeDegrees(card, frame)
    }
}

private class ClusterHolder(private val state: GauntletState) :
    RecyclerView.ViewHolder(LinearLayout(state.context)) {
    private val labels = Array(state.tier.depth + 1) {
        Array(3) {
            state.text(9f, 0xFFFFFFFF.toInt(), false).apply { background = rounded(0, state.context.dp(2f * state.s)) }
        }
    }

    init {
        var parent = itemView as LinearLayout
        parent.layoutParams = RecyclerView.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        for (remaining in state.tier.depth downTo 0) {
            val level = if (remaining == state.tier.depth) parent else LinearLayout(state.context)
            level.orientation = LinearLayout.VERTICAL
            level.background = rounded(LEVEL_BACKGROUND[remaining % 2], state.context.dp(4f * state.s))
            level.setPadding(state.px(3f), state.px(1f), state.px(1f), state.px(1f))
            val row = LinearLayout(state.context).apply { orientation = LinearLayout.HORIZONTAL }
            labels[remaining].forEachIndexed { chip, label ->
                row.addView(label, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply {
                    if (chip < 2) marginEnd = state.px(2f)
                })
            }
            level.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            if (level !== parent) {
                parent.addView(level, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                    topMargin = state.px(1f)
                })
            }
            parent = level
        }
    }

    fun bind(cluster: Int) {
        for (remaining in 0..state.tier.depth) {
            labels[remaining].forEachIndexed { chip, label ->
                label.text = "C$cluster.L$remaining.$chip"
                (label.background as GradientDrawable).setColor(PALETTE_ARGB[(remaining + chip + cluster) % PALETTE_ARGB.size])
            }
        }
    }
}

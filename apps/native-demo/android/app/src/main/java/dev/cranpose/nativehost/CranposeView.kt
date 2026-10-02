package dev.cranpose.nativehost

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import dev.cranpose.nativehost.generated.*
import java.lang.ref.WeakReference
import java.nio.ByteBuffer
import kotlinx.coroutines.*

class CranposeView(context: Context, nativeChildren: Boolean) : FrameLayout(context) {
    var onCount: (Int) -> Unit = {}
    var onError: (String) -> Unit = {}
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val handler = Handler(Looper.getMainLooper())
    private var bitmap: Bitmap? = null
    private val picture = object : View(context) {
        override fun onDraw(canvas: Canvas) {
            bitmap?.let { canvas.drawBitmap(it, 0f, 0f, null) }
        }
        override fun onTouchEvent(event: MotionEvent): Boolean = routeTouch(event)
    }
    private val children = mutableMapOf<ULong, WebChild>()
    private var inFlight = false
    private var pending = false
    private var scheduled = false
    private var closed = false
    private var pointerId = -1
    private val drawFrame = Runnable { scheduled = false; render() }
    private val session: DemoSession

    init {
        clipChildren = true
        clipToPadding = true
        addView(picture, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        val weakView = WeakReference(this)
        val callbackHandler = handler
        session = DemoSession(nativeChildren, object : FrameListener {
            override fun requestFrame() {
                callbackHandler.post { weakView.get()?.requestFrame() }
            }
        })
    }

    fun increment() = command { session.increment() }

    fun close() {
        if (closed) return
        closed = true
        handler.removeCallbacks(drawFrame)
        scope.cancel()
        session.shutdown()
        session.destroy()
        children.values.forEach { it.dispose() }
        children.clear()
        bitmap = null
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!closed) command { session.setVisible(true) }
    }

    override fun onDetachedFromWindow() {
        if (!closed) command { session.setVisible(false) }
        handler.removeCallbacks(drawFrame)
        scheduled = false
        pointerId = -1
        super.onDetachedFromWindow()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        if (isAttachedToWindow && !closed) command { session.setVisible(visibility == VISIBLE && isShown) }
    }

    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        if (isAttachedToWindow && !closed) command { session.setVisible(isShown && windowVisibility == VISIBLE) }
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        requestFrame()
    }

    fun requestFrame() {
        if (closed || !isAttachedToWindow || !isShown || windowVisibility != VISIBLE || width <= 0 || height <= 0) return
        pending = true
        if (inFlight || scheduled) return
        scheduled = true
        postOnAnimation(drawFrame)
    }

    private fun render() {
        if (closed || !isAttachedToWindow || !isShown || windowVisibility != VISIBLE || inFlight) return
        inFlight = true
        pending = false
        scope.launch {
            try {
                val frame = session.frame(width.toUInt(), height.toUInt(), resources.displayMetrics.density)
                if (closed) return@launch
                if (frame.pixels.isNotEmpty()) {
                    val current = bitmap
                    val next = if (current?.width == frame.width.toInt() && current.height == frame.height.toInt()) {
                        current
                    } else {
                        Bitmap.createBitmap(frame.width.toInt(), frame.height.toInt(), Bitmap.Config.ARGB_8888)
                            .also { it.density = Bitmap.DENSITY_NONE }
                    }
                    next.copyPixelsFromBuffer(ByteBuffer.wrap(frame.pixels))
                    bitmap = next
                    picture.invalidate()
                    reconcile(frame.slots)
                }
                onCount(frame.count)
                inFlight = false
                if (pending) requestFrame()
                else frame.nextFrameMs?.let {
                    scheduled = true
                    handler.postDelayed(drawFrame, it.toLong().coerceAtLeast(1))
                }
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                inFlight = false
                if (!closed) onError(error.message ?: error.toString())
            }
        }
    }

    private fun reconcile(slots: List<NativeSlot>) {
        val ids = slots.mapTo(mutableSetOf()) { it.id }
        val stale = children.keys.filter { it !in ids }
        stale.forEach { children.remove(it)?.dispose() }
        val density = resources.displayMetrics.density
        slots.forEach { slot ->
            require(slot.kind == "web") { "Unknown native view factory: ${slot.kind}" }
            val child = children.getOrPut(slot.id) {
                WebChild(context) { command { session.nativeEvent(slot.id, "increment") } }
                    .also { addView(it.view) }
            }
            val width = (slot.width * density).toInt().coerceAtLeast(0)
            val height = (slot.height * density).toInt().coerceAtLeast(0)
            val left = (slot.x * density).toInt()
            val top = (slot.y * density).toInt()
            val bounds = child.view.layoutParams as LayoutParams
            if (bounds.width != width || bounds.height != height ||
                bounds.leftMargin != left || bounds.topMargin != top) {
                bounds.width = width
                bounds.height = height
                bounds.leftMargin = left
                bounds.topMargin = top
                child.view.layoutParams = bounds
            }
            child.update(slot.value)
            child.view.bringToFront()
        }
    }

    private fun routeTouch(event: MotionEvent): Boolean {
        if (closed) return false
        val phase = when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> { pointerId = event.getPointerId(0); TouchPhase.DOWN }
            MotionEvent.ACTION_MOVE -> TouchPhase.MOVE
            MotionEvent.ACTION_UP -> TouchPhase.UP
            MotionEvent.ACTION_CANCEL -> TouchPhase.CANCEL
            MotionEvent.ACTION_POINTER_UP -> {
                if (event.getPointerId(event.actionIndex) != pointerId) return true
                TouchPhase.CANCEL
            }
            else -> return true
        }
        val index = event.findPointerIndex(pointerId)
        if (index < 0) return true
        val density = resources.displayMetrics.density
        command { session.touch(phase, event.getX(index) / density, event.getY(index) / density) }
        if (phase == TouchPhase.UP || phase == TouchPhase.CANCEL) pointerId = -1
        return true
    }

    private fun command(action: () -> Unit) {
        if (closed) return
        try { action(); requestFrame() }
        catch (error: Exception) { onError(error.message ?: error.toString()) }
    }
}

private class WebChild(context: Context, onIncrement: () -> Unit) {
    val view = WebView(context)
    private var html = ""
    init {
        view.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                if (request.url.toString() == "cranpose:increment") onIncrement()
                return true
            }
        }
    }
    fun update(value: String) {
        if (html == value) return
        html = value
        view.loadDataWithBaseURL(null, value, "text/html", "UTF-8", null)
    }
    fun dispose() {
        (view.parent as? android.view.ViewGroup)?.removeView(view)
        view.stopLoading()
        view.destroy()
    }
}

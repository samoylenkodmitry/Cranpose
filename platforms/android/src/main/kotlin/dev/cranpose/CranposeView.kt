package dev.cranpose

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import android.widget.FrameLayout
import uniffi.cranpose_native.*
import java.lang.ref.WeakReference
import java.nio.ByteBuffer
import kotlinx.coroutines.*

/** A native view that owns a Cranpose session and its embedded native children. */
class CranposeView(
    context: Context,
    private val session: NativeSession,
    factories: Map<String, NativeViewFactory> = emptyMap(),
) : FrameLayout(context), AutoCloseable {
    /** Application events emitted by the Rust composition. */
    var onEvent: (NativeEvent) -> Unit = {}
    /** Rendering, native factory, and transport failures. */
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
    private val registry = NativeViewRegistry(this, factories) { id, event ->
        command { session.nativeEvent(id, event) }
    }
    private var contentVisible = false
    private var inFlight = false
    private var pending = false
    private var scheduled = false
    private var closed = false
    private var pointerId = -1
    private val drawFrame = Runnable { scheduled = false; render() }
    private val wakeFrame = Runnable { requestFrame() }

    init {
        clipChildren = true
        clipToPadding = true
        addView(picture, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        val weakView = WeakReference(this)
        val callbackHandler = handler
        session.setListener(object : FrameListener {
            override fun requestFrame() {
                callbackHandler.post { weakView.get()?.requestFrame() }
            }
        })
    }

    /** Sends application input to Rust. */
    fun sendEvent(name: String, value: String = "") = command { session.sendEvent(name, value) }

    /** Releases this view's session and every native child. Idempotent. */
    override fun close() {
        if (closed) return
        closed = true
        removeCallbacks(drawFrame)
        removeCallbacks(wakeFrame)
        scope.cancel()
        session.shutdown()
        session.destroy()
        registry.close()
        bitmap = null
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        updateVisibility()
    }

    override fun onDetachedFromWindow() {
        updateVisibility(false)
        removeCallbacks(drawFrame)
        removeCallbacks(wakeFrame)
        scheduled = false
        pointerId = -1
        super.onDetachedFromWindow()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        updateVisibility()
    }

    override fun onVisibilityChanged(changedView: View, visibility: Int) {
        super.onVisibilityChanged(changedView, visibility)
        updateVisibility()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        requestFrame()
    }

    private fun updateVisibility(visible: Boolean = isAttachedToWindow && isShown && windowVisibility == VISIBLE) {
        if (closed || visible == contentVisible) return
        contentVisible = visible
        registry.setVisible(visible)
        command { session.setVisible(visible) }
        if (!visible) {
            removeCallbacks(drawFrame)
            removeCallbacks(wakeFrame)
            scheduled = false
            pointerId = -1
        }
    }

    private fun requestFrame() {
        if (closed || !isAttachedToWindow || !isShown || windowVisibility != VISIBLE || width <= 0 || height <= 0) return
        pending = true
        removeCallbacks(wakeFrame)
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
                    registry.reconcile(frame.slots)
                }
                frame.events.forEach(onEvent)
                inFlight = false
                if (closed || !contentVisible) return@launch
                if (pending || frame.nextFrameMs == 0UL) requestFrame()
                else frame.nextFrameMs?.let {
                    postDelayed(wakeFrame, it.toLong().coerceAtLeast(1))
                }
            } catch (error: CancellationException) {
                throw error
            } catch (error: Exception) {
                inFlight = false
                if (!closed) onError(error.message ?: error.toString())
            }
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

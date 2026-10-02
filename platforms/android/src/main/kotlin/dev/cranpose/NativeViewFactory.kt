package dev.cranpose

import android.content.Context
import android.view.View
import android.widget.FrameLayout
import uniffi.cranpose_native.NativeSlot

/** Creates one child for a native slot. Register the factory under the slot's kind. */
fun interface NativeViewFactory {
    /** Creates a view and connects events to its owning Rust slot. */
    fun create(context: Context, emit: (String) -> Unit): NativeViewChild
}

/** Lifecycle of one platform child. Configuration changes preserve this instance. */
interface NativeViewChild {
    /** Native content displayed above the Cranpose image. */
    val view: View
    /** Applies the latest slot configuration. */
    fun update(value: String)
    /** Updates attachment and visibility state. */
    fun setVisible(visible: Boolean) { view.visibility = if (visible) View.VISIBLE else View.INVISIBLE }
    /** Releases platform resources. */
    fun dispose()
}

internal class NativeViewRegistry(
    private val parent: FrameLayout,
    private val factories: Map<String, NativeViewFactory>,
    private val emit: (ULong, String) -> Unit,
) {
    private class Entry(val kind: String, val child: NativeViewChild, var seen: Boolean = false)
    private val children = mutableMapOf<ULong, Entry>()
    private var visible = true

    fun reconcile(slots: List<NativeSlot>) {
        slots.forEach { require(factories.containsKey(it.kind)) { "Unknown native view factory: ${it.kind}" } }
        children.values.forEach { it.seen = false }
        val density = parent.resources.displayMetrics.density
        slots.forEach { slot ->
            var entry = children[slot.id]
            if (entry != null && entry.kind != slot.kind) {
                remove(entry)
                children.remove(slot.id)
                entry = null
            }
            if (entry == null) {
                val factory = requireNotNull(factories[slot.kind])
                entry = Entry(slot.kind, factory.create(parent.context) { emit(slot.id, it) })
                children[slot.id] = entry
                parent.addView(entry.child.view)
                entry.child.setVisible(visible)
            }
            entry.seen = true
            val view = entry.child.view
            val width = (slot.width * density).toInt().coerceAtLeast(0)
            val height = (slot.height * density).toInt().coerceAtLeast(0)
            val left = (slot.x * density).toInt()
            val top = (slot.y * density).toInt()
            val bounds = view.layoutParams as FrameLayout.LayoutParams
            if (bounds.width != width || bounds.height != height || bounds.leftMargin != left || bounds.topMargin != top) {
                bounds.width = width
                bounds.height = height
                bounds.leftMargin = left
                bounds.topMargin = top
                view.layoutParams = bounds
            }
            entry.child.update(slot.value)
            view.bringToFront()
        }
        val iterator = children.values.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            if (!entry.seen) { remove(entry); iterator.remove() }
        }
    }

    fun setVisible(value: Boolean) {
        visible = value
        children.values.forEach { it.child.setVisible(value) }
    }

    private fun remove(entry: Entry) {
        parent.removeView(entry.child.view)
        entry.child.dispose()
    }

    fun close() {
        children.values.forEach(::remove)
        children.clear()
    }
}

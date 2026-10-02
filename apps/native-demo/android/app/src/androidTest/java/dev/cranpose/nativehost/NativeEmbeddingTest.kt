package dev.cranpose.nativehost

import dev.cranpose.CranposeView
import android.content.Intent
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.webkit.WebView
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NativeEmbeddingTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    @Test
    fun nativeRustAndWebViewExchangeEvents() {
        val activity = instrumentation.startActivitySync(Intent(
            instrumentation.targetContext, MainActivity::class.java
        ).putExtra("native-component", true).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) as MainActivity
        try {
            awaitCondition { onMain { activity.window.decorView.hasWindowFocus() } }
            awaitCount(activity, 0)
            nativeClick(activity, "native-increment")
            awaitCount(activity, 1)
            tapRust(activity, 60f, 96f)
            awaitCount(activity, 2)
            nativeClick(activity, "switch-mode")
            awaitCount(activity, 0)
            awaitCondition { containsText(instrumentation.uiAutomation.rootInActiveWindow, "Learn more") }
            val originalWeb = onMain { views(activity.window.decorView).filterIsInstance<WebView>().first() }
            assertTrue(onMain { originalWeb.url?.startsWith("https://example.com") == true })
            val marker = java.util.concurrent.CountDownLatch(1)
            instrumentation.runOnMainSync {
                originalWeb.evaluateJavascript("window.cranposeMarker = 'preserved'") { marker.countDown() }
            }
            assertTrue(marker.await(5, java.util.concurrent.TimeUnit.SECONDS))
            nativeClick(activity, "native-increment")
            awaitCount(activity, 1)
            assertSame(originalWeb, onMain { views(activity.window.decorView).filterIsInstance<WebView>().first() })
            val component = onMain { views(activity.window.decorView).filterIsInstance<CranposeView>().first() }
            instrumentation.runOnMainSync {
                component.visibility = View.INVISIBLE
                component.sendEvent("increment")
            }
            SystemClock.sleep(100)
            instrumentation.runOnMainSync { component.visibility = View.VISIBLE }
            awaitCount(activity, 2)
            assertSame(originalWeb, onMain { views(activity.window.decorView).filterIsInstance<WebView>().first() })
            val checked = java.util.concurrent.CountDownLatch(1)
            var value: String? = null
            instrumentation.runOnMainSync {
                originalWeb.evaluateJavascript("window.cranposeMarker") { value = it; checked.countDown() }
            }
            assertTrue(checked.await(5, java.util.concurrent.TimeUnit.SECONDS))
            assertEquals("\"preserved\"", value)
            tapRust(activity, 170f, 96f)
            awaitCondition { onMain { views(activity.window.decorView).none { it is WebView } } }
            tapRust(activity, 170f, 96f)
            awaitCondition { onMain { views(activity.window.decorView).any { it is WebView } } }
            nativeClick(activity, "switch-mode")
            awaitCondition { onMain { views(activity.window.decorView).none { it is WebView } } }
        } finally { instrumentation.runOnMainSync { activity.finish() } }
    }

    private fun awaitCount(activity: MainActivity, value: Int) = awaitCondition {
        onMain {
            views(activity.window.decorView).filterIsInstance<TextView>()
                .any { it.tag == "host-count" && it.text.toString() == "Host received: $value" }
        }
    }

    private fun nativeClick(activity: MainActivity, description: String) {
        instrumentation.runOnMainSync {
            views(activity.window.decorView).first { it.contentDescription == description }.performClick()
        }
    }

    private fun tapRust(activity: MainActivity, x: Float, y: Float) {
        val point = onMain {
            val view = views(activity.window.decorView).filterIsInstance<CranposeView>().first()
            val position = IntArray(2)
            view.getLocationOnScreen(position)
            val density = view.resources.displayMetrics.density
            floatArrayOf(position[0] + x * density, position[1] + y * density)
        }
        tapAt(point[0], point[1])
    }

    private fun tapAt(x: Float, y: Float) {
        val time = SystemClock.uptimeMillis()
        for (action in intArrayOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
            val event = MotionEvent.obtain(time, SystemClock.uptimeMillis(), action, x, y, 0)
            assertTrue(instrumentation.uiAutomation.injectInputEvent(event, true))
            event.recycle()
        }
    }

    private fun views(root: View): Sequence<View> = sequence {
        yield(root)
        if (root is ViewGroup) for (index in 0 until root.childCount) yieldAll(views(root.getChildAt(index)))
    }

    private fun containsText(node: AccessibilityNodeInfo?, expected: String): Boolean {
        if (node == null) return false
        if (node.text?.toString() == expected) return true
        for (index in 0 until node.childCount) if (containsText(node.getChild(index), expected)) return true
        return false
    }

    private fun <T> onMain(block: () -> T): T {
        val value = java.util.concurrent.FutureTask(block)
        instrumentation.runOnMainSync(value)
        return value.get()
    }

    private fun awaitCondition(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 20_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(50)
        }
        fail("Native embedding condition was not satisfied")
    }
}

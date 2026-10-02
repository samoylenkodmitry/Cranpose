package dev.cranpose.nativehost

import dev.cranpose.CranposeView
import dev.cranpose.WebViewFactory
import uniffi.cranpose_native_demo.createDemoComponent
import android.app.Activity
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {
    private lateinit var column: LinearLayout
    private lateinit var count: TextView
    private var component: CranposeView? = null
    private var nativeChildren = true

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        nativeChildren = !intent.getBooleanExtra("native-component", false)
        column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val padding = (16 * resources.displayMetrics.density).toInt()
            setPadding(padding, padding, padding, padding)
            setOnApplyWindowInsetsListener { view, insets ->
                val bars = if (android.os.Build.VERSION.SDK_INT >= 30) {
                    val current = insets.getInsets(android.view.WindowInsets.Type.systemBars())
                    android.graphics.Rect(current.left, current.top, current.right, current.bottom)
                } else {
                    @Suppress("DEPRECATION")
                    android.graphics.Rect(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                        insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
                }
                view.setPadding(padding + bars.left, padding + bars.top,
                    padding + bars.right, padding + bars.bottom)
                insets
            }
        }
        setContentView(column)
        column.addView(TextView(this).apply { text = "Cranpose + Android"; textSize = 28f })
        column.addView(Button(this).apply {
            text = if (nativeChildren) "Switch: native contains Cranpose" else "Switch: Cranpose contains native"
            contentDescription = "switch-mode"
            setOnClickListener {
                nativeChildren = !nativeChildren
                text = if (nativeChildren) "Switch: native contains Cranpose" else "Switch: Cranpose contains native"
                mount()
            }
        })
        column.addView(Button(this).apply {
            text = "Add from native Android"
            contentDescription = "native-increment"
            setOnClickListener { component?.sendEvent("increment") }
        })
        count = TextView(this).apply { text = "Host received: 0"; textSize = 18f; tag = "host-count" }
        column.addView(count)
        mount()
    }

    private fun mount() {
        component?.let { it.close(); column.removeView(it) }
        val child = CranposeView(this, createDemoComponent(nativeChildren, intent.getStringExtra("website") ?: "https://example.com"),
            mapOf("web" to WebViewFactory()))
        child.onEvent = { if (it.name == "count") count.text = "Host received: ${it.value}" }
        child.onError = { count.text = it }
        val height = if (nativeChildren) 0 else (260 * resources.displayMetrics.density).toInt()
        column.addView(child, LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT, height, if (nativeChildren) 1f else 0f))
        component = child
    }

    override fun onDestroy() {
        component?.close()
        super.onDestroy()
    }
}

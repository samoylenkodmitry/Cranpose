package dev.perfcompare.views

import android.app.Activity
import android.os.Bundle
import android.util.Log
import android.view.Gravity
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.widget.LinearLayout
import dev.perfcompare.shared.gauntletTier

const val TAG = "PerfCompare"

/** The gauntlet in Android Views: `--es scenario gauntlet --ei tier N [--ei freeze K]`. */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val fonts = Fonts.load()
        val state = GauntletState(this, gauntletTier(intent.getIntExtra("tier", 5)), fonts)
        val screen = GauntletScreen(state, intent.getIntExtra("freeze", 0))
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(0xFFEEF0F5.toInt())
            addView(GauntletText(this@MainActivity).styled(20f, 0xFFFFFFFF.toInt(), true, fonts).apply {
                text = "Gauntlet"
                gravity = Gravity.CENTER_VERTICAL
                setBackgroundColor(0xFF1E2A4A.toInt())
                setPadding(px(16f), 0, px(16f), 0)
            }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, px(56f)))
            addView(screen.root, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        }
        setContentView(content)
        content.viewTreeObserver.addOnDrawListener(object : ViewTreeObserver.OnDrawListener {
            private var logged = false
            override fun onDraw() {
                if (!logged) {
                    logged = true
                    Log.i(TAG, "PERF first_frame")
                }
            }
        })
        screen.start()
    }
}

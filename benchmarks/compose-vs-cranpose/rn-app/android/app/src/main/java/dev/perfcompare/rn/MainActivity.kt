package dev.perfcompare.rn

import android.os.Bundle
import android.util.Log
import android.view.ViewTreeObserver
import com.facebook.react.ReactActivity
import com.facebook.react.ReactActivityDelegate
import com.facebook.react.defaults.DefaultNewArchitectureEntryPoint.fabricEnabled
import com.facebook.react.defaults.DefaultReactActivityDelegate

const val TAG = "PerfCompare"

/**
 * The gauntlet in React Native: `--es scenario gauntlet --ei tier N [--ei freeze K]`.
 * The extras reach the root component as its `tier` and `freeze` props.
 */
class MainActivity : ReactActivity() {
    override fun getMainComponentName(): String = "PerfRn"

    override fun createReactActivityDelegate(): ReactActivityDelegate =
        object : DefaultReactActivityDelegate(this, mainComponentName, fabricEnabled) {
            override fun getLaunchOptions() = Bundle().apply {
                putInt("tier", intent.getIntExtra("tier", 5))
                putInt("freeze", intent.getIntExtra("freeze", 0))
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.viewTreeObserver.addOnDrawListener(object : ViewTreeObserver.OnDrawListener {
            private var logged = false

            override fun onDraw() {
                if (!logged) {
                    logged = true
                    Log.i(TAG, "PERF first_frame")
                }
            }
        })
    }
}

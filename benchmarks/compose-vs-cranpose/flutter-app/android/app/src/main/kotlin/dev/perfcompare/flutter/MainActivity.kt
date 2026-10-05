package dev.perfcompare.flutter

import android.util.Log
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.renderer.FlutterUiDisplayListener
import io.flutter.plugin.common.MethodChannel

const val TAG = "PerfCompare"

/**
 * The gauntlet in Flutter: `--es scenario gauntlet --ei tier N [--ei freeze K]`.
 * The extras reach Dart's `main` as its arguments, and Dart's `PERF` lines
 * reach the log under the tag the other apps use.
 */
class MainActivity : FlutterActivity() {
    override fun getDartEntrypointArgs(): List<String> =
        listOf(intent.getIntExtra("tier", 5).toString(), intent.getIntExtra("freeze", 0).toString())

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "perfcompare/log")
            .setMethodCallHandler { call, result ->
                Log.i(TAG, call.arguments.toString())
                result.success(null)
            }
        val renderer = flutterEngine.renderer
        renderer.addIsDisplayingFlutterUiListener(object : FlutterUiDisplayListener {
            override fun onFlutterUiDisplayed() {
                Log.i(TAG, "PERF first_frame")
                renderer.removeIsDisplayingFlutterUiListener(this)
            }

            override fun onFlutterUiNoLongerDisplayed() = Unit
        })
    }
}

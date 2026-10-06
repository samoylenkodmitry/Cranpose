package dev.perfcompare.lynx

import android.app.Activity
import android.os.Bundle
import android.util.Log
import com.lynx.tasm.LynxViewBuilder
import com.lynx.tasm.LynxViewClient
import com.lynx.tasm.TemplateData
import com.lynx.tasm.ThreadStrategyForRendering
import com.lynx.xelement.XElementBehaviors

/** The gauntlet in Lynx: `--es scenario gauntlet --ei tier N [--ei freeze K]`. */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Layout on Lynx's own thread: with all of it on the UI thread, the
        // default, frames from tier 6 on outlast a vsync and the window never
        // reports its first draw.
        val view = LynxViewBuilder()
            .addBehaviors(XElementBehaviors().create())
            .setThreadStrategyForRendering(ThreadStrategyForRendering.PART_ON_LAYOUT)
            .build(this)
        setContentView(view)
        view.addLynxViewClient(object : LynxViewClient() {
            override fun onFirstScreen() {
                Log.i(TAG, "PERF first_frame")
            }
        })
        val bundle = assets.open("main.lynx.bundle").use { it.readBytes() }
        val extras = mapOf<String, Any>(
            "tier" to intent.getIntExtra("tier", 5),
            "freeze" to intent.getIntExtra("freeze", 0),
        )
        view.renderTemplate(bundle, TemplateData.fromMap(extras))
    }
}

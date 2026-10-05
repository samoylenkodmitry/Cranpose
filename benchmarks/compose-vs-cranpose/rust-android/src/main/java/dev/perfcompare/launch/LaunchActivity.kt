package dev.perfcompare.launch

import android.app.NativeActivity
import android.os.Bundle
import java.io.File

/**
 * The gauntlet in a Rust framework: `--es scenario gauntlet --ei tier N [--ei freeze K]`.
 * The extras reach the native side as `launch.txt` in the app's files,
 * written before it starts.
 */
class LaunchActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        File(filesDir, "launch.txt").writeText("${intent.getIntExtra("tier", 5)} ${intent.getIntExtra("freeze", 0)}")
        super.onCreate(savedInstanceState)
    }
}

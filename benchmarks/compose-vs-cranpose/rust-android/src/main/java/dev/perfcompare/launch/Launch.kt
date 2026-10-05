package dev.perfcompare.launch

import android.app.Activity
import java.io.File

/**
 * Hands the native side the `am start` extras, `--ei tier N [--ei freeze K]`,
 * as `launch.txt` in the app's files: called before the native side starts.
 */
fun Activity.writeLaunch() {
    File(filesDir, "launch.txt").writeText("${intent.getIntExtra("tier", 5)} ${intent.getIntExtra("freeze", 0)}")
}

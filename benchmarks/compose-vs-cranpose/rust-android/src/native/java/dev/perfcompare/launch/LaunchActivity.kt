package dev.perfcompare.launch

import android.app.NativeActivity
import android.os.Bundle

/** The gauntlet in a Rust framework that runs in a NativeActivity. */
class LaunchActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        writeLaunch()
        super.onCreate(savedInstanceState)
    }
}

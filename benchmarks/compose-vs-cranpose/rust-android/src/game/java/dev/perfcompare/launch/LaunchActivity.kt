package dev.perfcompare.launch

import android.os.Bundle
import com.google.androidgamesdk.GameActivity

/** The gauntlet in a Rust framework that runs in a GameActivity. */
class LaunchActivity : GameActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        writeLaunch()
        super.onCreate(savedInstanceState)
    }
}

package dev.perfcompare.compose

import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color

const val TAG = "PerfCompare"

enum class Scenario(val title: String) {
    Feed("Compose · feed"),
    Ticker("Compose · ticker"),
    Particles("Compose · particles"),
    Layers("Compose · layers"),
    Grid("Compose · grid"),
    GridLayer("Compose · grid + layers"),
    Deep("Compose · deep"),
    DeepLayer("Compose · deep + layers");

    companion object {
        fun fromName(name: String?): Scenario = when (name) {
            "ticker" -> Ticker
            "particles" -> Particles
            "layers" -> Layers
            "grid" -> Grid
            "grid_layer" -> GridLayer
            "deep" -> Deep
            "deep_layer" -> DeepLayer
            else -> Feed
        }
    }
}

/** What `am start` asked for: the scenario and its knobs. */
data class Launch(
    val scenario: Scenario,
    val feed: FeedLoad,
    val quotes: Int,
    val particles: Int,
    val opaque: Boolean,
    val tileColumns: Int,
    val tileRows: Int,
    val rows: Int,
    val columns: Int,
    val depth: Int,
    val chips: Int,
)

private var firstFrameLogged = false

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val launch = Launch(
            scenario = Scenario.fromName(intent.getStringExtra("scenario")),
            feed = FeedLoad(
                speed = if (intent.getBooleanExtra("still", false)) 0f else intent.getFloatExtra("speed", 2000f),
                columns = intent.getIntExtra("fcols", 1).coerceAtLeast(1),
                comments = intent.getIntExtra("comments", 0),
                scale = intent.getFloatExtra("ui", 1f),
            ),
            quotes = intent.getIntExtra("quotes", 24),
            particles = intent.getIntExtra("particles", 2000),
            opaque = intent.getBooleanExtra("opaque", false),
            tileColumns = intent.getIntExtra("tcols", 6),
            tileRows = intent.getIntExtra("trows", 11),
            rows = intent.getIntExtra("rows", 30),
            columns = intent.getIntExtra("cols", 12),
            depth = intent.getIntExtra("depth", 40),
            chips = intent.getIntExtra("chips", 6),
        )
        setContent { PerfCompareApp(launch) }
    }
}

@Composable
fun PerfCompareApp(launch: Launch) {
    Column(
        Modifier
            .fillMaxSize()
            .background(Color(0xFFEEF0F5))
            .drawBehind {
                if (!firstFrameLogged) {
                    firstFrameLogged = true
                    Log.i(TAG, "PERF first_frame")
                }
            },
    ) {
        TopBar(launch.scenario.title)
        when (launch.scenario) {
            Scenario.Feed -> FeedScreen(launch.feed)
            Scenario.Ticker -> TickerScreen(launch.quotes)
            Scenario.Particles -> ParticlesScreen(launch.particles)
            Scenario.Layers -> LayersScreen(launch.opaque, launch.tileColumns, launch.tileRows)
            Scenario.Grid, Scenario.GridLayer ->
                GridScreen(launch.scenario == Scenario.GridLayer, launch.rows, launch.columns)
            Scenario.Deep, Scenario.DeepLayer ->
                DeepScreen(launch.scenario == Scenario.DeepLayer, launch.depth, launch.chips)
        }
    }
}

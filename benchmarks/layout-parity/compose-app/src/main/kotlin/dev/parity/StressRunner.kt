@file:OptIn(ExperimentalFoundationApi::class)

package dev.parity

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.staggeredgrid.LazyVerticalStaggeredGrid
import androidx.compose.foundation.lazy.staggeredgrid.StaggeredGridCells
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PageSize
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.zIndex
import java.io.File

private const val FRAME_NS = 16_666_667L

private data class ScenarioOutcome(
    val name: String,
    val passed: Boolean,
    val error: String?,
)

private fun escapeJson(s: String): String =
    s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")

private fun writeStressJson(file: File, outcomes: List<ScenarioOutcome>) {
    file.parentFile?.mkdirs()
    val sb = StringBuilder()
    sb.append("{\n  \"framework\": \"compose\",\n  \"scenarios\": [\n")
    outcomes.forEachIndexed { idx, o ->
        val comma = if (idx + 1 < outcomes.size) "," else ""
        val errJson = o.error?.let { "\"${escapeJson(it)}\"" } ?: "null"
        sb.append("    {\"name\": \"${escapeJson(o.name)}\", \"passed\": ${o.passed}, \"error\": $errJson}$comma\n")
    }
    sb.append("  ]\n}\n")
    file.writeText(sb.toString())
}

private fun runScenario(name: String, block: () -> Unit): ScenarioOutcome {
    return try {
        block()
        println("  [PASS] $name")
        ScenarioOutcome(name, true, null)
    } catch (t: Throwable) {
        val msg = "${t::class.simpleName}: ${t.message}"
        println("  [FAIL] $name -> $msg")
        ScenarioOutcome(name, false, msg)
    }
}

private fun scenarioDropFullAtlas(fontFamily: FontFamily) {
    val tracker = BoundsTracker()
    val scene = ImageComposeScene(1760, 980, Density(1f)) {
        ParityAtlasScreen(fontFamily, tracker, 1760, 980, "1.0")
    }
    scene.render(0L)
    scene.render(FRAME_NS)
    scene.close()
}

private fun scenarioToggleScrollMount() {
    val show = mutableStateOf(true)
    val scene = ImageComposeScene(400, 300, Density(1f)) {
        if (show.value) {
            val vs = rememberScrollState(20)
            val hs = rememberScrollState(15)
            Column(Modifier.fillMaxSize().verticalScroll(vs)) {
                Row(Modifier.fillMaxWidth().horizontalScroll(hs)) {
                    repeat(12) { Box(Modifier.size(60.dp, 40.dp).background(Color.Blue)) }
                }
                repeat(20) { Box(Modifier.fillMaxWidth().height(30.dp).background(Color.Gray)) }
            }
        } else {
            Box(Modifier.fillMaxSize())
        }
    }
    repeat(10) { i ->
        show.value = (i % 2 == 0)
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

private fun scenarioSwapScrollState() {
    val useA = mutableStateOf(true)
    val scene = ImageComposeScene(300, 200, Density(1f)) {
        val a = rememberScrollState(10)
        val b = rememberScrollState(50)
        Column(Modifier.fillMaxSize().verticalScroll(if (useA.value) a else b)) {
            repeat(20) { Box(Modifier.fillMaxWidth().height(25.dp)) }
        }
    }
    repeat(8) { i ->
        useA.value = (i % 2 == 0)
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

private fun scenarioLazyColumnNestedScrolls(fontFamily: FontFamily) {
    val jump = mutableIntStateOf(0)
    val scene = ImageComposeScene(360, 240, Density(1f)) {
        val outer = rememberLazyListState()
        val target = jump.intValue
        LazyColumn(Modifier.fillMaxSize(), state = outer) {
            items(40, key = { it }) { idx ->
                when (idx % 5) {
                    0 -> {
                        val hs = rememberScrollState(12)
                        Row(Modifier.fillMaxWidth().horizontalScroll(hs)) {
                            repeat(8) { Box(Modifier.size(60.dp, 24.dp)) }
                        }
                    }
                    1 -> {
                        val lr = rememberLazyListState()
                        LazyRow(Modifier.fillMaxWidth(), state = lr) {
                            items(8) { Box(Modifier.size(50.dp, (18 + it * 2).dp)) }
                        }
                    }
                    2 -> {
                        val lr = rememberLazyListState()
                        LazyRow(Modifier.fillMaxWidth().height(32.dp), state = lr) {
                            items(8) { Box(Modifier.size(50.dp, 24.dp)) }
                        }
                    }
                    3 -> {
                        FlowRow(Modifier.fillMaxWidth()) {
                            repeat(6) { Box(Modifier.size(70.dp, 18.dp)) }
                        }
                    }
                    else -> {
                        BoxWithConstraints(Modifier.fillMaxWidth()) {
                            val mw = this.maxWidth
                            Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min)) {
                                BasicText(
                                    "Item $idx target=$target w=${mw.value.toInt()}",
                                    style = TextStyle(fontSize = 10.sp, fontFamily = fontFamily),
                                )
                                Box(Modifier.width(2.dp).fillMaxHeight().background(Color.Red))
                            }
                        }
                    }
                }
            }
        }
        outer.dispatchRawDelta(if (target % 2 == 0) 320f else -240f)
    }
    repeat(10) { i ->
        jump.intValue = i
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

private fun scenarioViewportResizeExtremes(fontFamily: FontFamily) {
    val tracker = BoundsTracker()
    val scene = ImageComposeScene(1760, 980, Density(1f)) {
        ParityAtlasScreen(fontFamily, tracker, 1760, 980, "1.0")
    }
    val sizes = listOf(
        IntSize(1760, 980),
        IntSize(640, 480),
        IntSize(160, 160),
        IntSize(1, 1),
        IntSize(1760, 980),
    )
    sizes.forEachIndexed { idx, sz ->
        scene.constraints = Constraints.fixed(sz.width, sz.height)
        scene.render(idx * FRAME_NS)
    }
    scene.close()
}

private fun scenarioDynamicRtlFlip(fontFamily: FontFamily) {
    val dir = mutableStateOf(LayoutDirection.Ltr)
    val tracker = BoundsTracker()
    val scene = ImageComposeScene(1760, 980, Density(1f)) {
        CompositionLocalProvider(LocalLayoutDirection provides dir.value) {
            ParityAtlasScreen(fontFamily, tracker, 1760, 980, "1.0")
        }
    }
    repeat(6) { i ->
        dir.value = if (i % 2 == 0) LayoutDirection.Ltr else LayoutDirection.Rtl
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

private fun scenarioHotModifierPermutation() {
    val step = mutableIntStateOf(0)
    val scene = ImageComposeScene(300, 200, Density(1f)) {
        val s = rememberScrollState(5)
        val m = when (step.intValue % 4) {
            0 -> Modifier.padding(4.dp).verticalScroll(s).background(Color.Blue).size(120.dp, 80.dp)
            1 -> Modifier.size(120.dp, 80.dp).graphicsLayer { rotationZ = 5f }.offset(4.dp, 2.dp).clipToBounds()
            2 -> Modifier.zIndex(2f).border(1.dp, Color.Red, RectangleShape).horizontalScroll(s).padding(6.dp)
            else -> Modifier.padding(2.dp).background(Color.Green)
        }
        Box(m) { Box(Modifier.size(200.dp, 200.dp)) }
    }
    repeat(12) { i ->
        step.intValue = i
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

private fun scenarioSubcomposeAndLazyChurn() {
    val tick = mutableIntStateOf(0)
    val scene = ImageComposeScene(320, 240, Density(1f)) {
        val t = tick.intValue
        SubcomposeLayout(Modifier.fillMaxSize()) { constraints ->
            val ms = subcompose("slot_${t % 3}") {
                val vs = rememberScrollState(8)
                val gs = remember { t % 2 == 0 }
                if (gs) {
                    LazyVerticalGrid(GridCells.Fixed(2), Modifier.fillMaxSize()) {
                        items(if (t % 4 == 0) 0 else 10) { Box(Modifier.height(24.dp)) }
                    }
                } else {
                    Column(Modifier.fillMaxSize().verticalScroll(vs)) {
                        val ps = rememberPagerState(0) { 3 }
                        HorizontalPager(
                            ps,
                            Modifier.fillMaxWidth().height(40.dp),
                            pageSize = PageSize.Fixed(80.dp),
                        ) { Box(Modifier.fillMaxSize()) }
                        LazyVerticalStaggeredGrid(
                            StaggeredGridCells.Fixed(2),
                            Modifier.fillMaxWidth().height(80.dp),
                        ) { items(6) { Box(Modifier.height((16 + it * 4).dp)) } }
                    }
                }
            }
            val ps = ms.map { it.measure(Constraints.fixed(constraints.maxWidth, constraints.maxHeight)) }
            layout(constraints.maxWidth, constraints.maxHeight) { ps.forEach { it.place(0, 0) } }
        }
    }
    repeat(12) { i ->
        tick.intValue = i
        scene.render(i * FRAME_NS)
    }
    scene.close()
}

fun runComposeStressSuite(fontFamily: FontFamily, jsonOut: File? = null): Int {
    println("=== Jetpack Compose Behavioral & No-Panic Stress Suite ===")
    val scenarios: List<Pair<String, () -> Unit>> = listOf(
        "drop_scene_with_scroll_nodes" to { scenarioDropFullAtlas(fontFamily) },
        "toggle_scroll_mount_unmount" to { scenarioToggleScrollMount() },
        "swap_scroll_state_on_live_node" to { scenarioSwapScrollState() },
        "lazy_column_scroll_detaches_nested_scrolls" to { scenarioLazyColumnNestedScrolls(fontFamily) },
        "viewport_resize_extremes" to { scenarioViewportResizeExtremes(fontFamily) },
        "dynamic_rtl_ltr_flip" to { scenarioDynamicRtlFlip(fontFamily) },
        "modifier_chain_hot_permutation" to { scenarioHotModifierPermutation() },
        "subcompose_and_lazy_toggle_churn" to { scenarioSubcomposeAndLazyChurn() },
    )
    val outcomes = scenarios.map { (name, block) -> runScenario(name, block) }
    val passed = outcomes.count { it.passed }
    if (jsonOut != null) {
        writeStressJson(jsonOut, outcomes)
    }
    println("Compose stress summary: $passed/${outcomes.size} passed")
    return if (passed == outcomes.size) 0 else 1
}

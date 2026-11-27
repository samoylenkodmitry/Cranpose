package dev.parity

import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import org.jetbrains.skia.EncodedImageFormat
import java.io.File
import java.util.Locale
import kotlin.math.roundToInt
import kotlin.system.exitProcess

private fun resolveFontsDir(args: Array<String>): File {
    val explicit = argValue(args, "fonts")
        ?: System.getProperty("parity.fontsDir")
        ?: "../compose-vs-cranpose/fonts"
    return File(explicit)
}

private fun loadRoboto(fontsDir: File): FontFamily = FontFamily(
    Font(File(fontsDir, "Roboto-Regular.ttf"), FontWeight.Normal),
    Font(File(fontsDir, "Roboto-Medium.ttf"), FontWeight.Medium),
    Font(File(fontsDir, "Roboto-Bold.ttf"), FontWeight.Bold),
)

private fun argValue(args: Array<String>, name: String): String? {
    val prefix = "--$name="
    for (i in args.indices) {
        if (args[i].startsWith(prefix)) return args[i].removePrefix(prefix)
        if (args[i] == "--$name" && i + 1 < args.size) return args[i + 1]
    }
    return null
}

private fun formatDensity(d: Float): String =
    String.format(Locale.US, "%.2f", d)
        .trimEnd('0')
        .trimEnd('.')
        .let { if (it.contains('.')) it else "$it.0" }

private fun writeBoundsJson(
    file: File,
    widthDp: Int,
    heightDp: Int,
    density: Float,
    tracker: BoundsTracker,
) {
    val sb = StringBuilder()
    sb.append("{\n")
    sb.append("  \"framework\": \"compose\",\n")
    sb.append("  \"width_dp\": ").append(widthDp).append(",\n")
    sb.append("  \"height_dp\": ").append(heightDp).append(",\n")
    sb.append("  \"density\": ").append(String.format(Locale.US, "%.4f", density)).append(",\n")
    sb.append("  \"nodes\": {\n")
    val list = tracker.entries.entries.toList()
    list.forEachIndexed { idx, (tag, r) ->
        val comma = if (idx + 1 < list.size) "," else ""
        sb.append(
            String.format(
                Locale.US,
                "    \"%s\": {\"x\": %.4f, \"y\": %.4f, \"width\": %.4f, \"height\": %.4f}%s\n",
                tag, r.x, r.y, r.width, r.height, comma,
            )
        )
    }
    sb.append("  }\n}\n")
    file.writeText(sb.toString())
}

private fun runHeadless(
    outDir: File,
    widthDp: Int,
    heightDp: Int,
    density: Float,
    densityLabel: String,
    roboto: FontFamily,
) {
    outDir.mkdirs()
    val tracker = BoundsTracker()
    val pxW = (widthDp * density).roundToInt()
    val pxH = (heightDp * density).roundToInt()
    val scene = ImageComposeScene(
        width = pxW,
        height = pxH,
        density = Density(density = density, fontScale = 1.0f),
    ) {
        ParityAtlasScreen(
            fontFamily = roboto,
            tracker = tracker,
            viewportWidthDp = widthDp,
            viewportHeightDp = heightDp,
            densityLabel = densityLabel,
        )
    }
    scene.render(0L)
    val image = scene.render(16_666_667L)
    val pngBytes = image.encodeToData(EncodedImageFormat.PNG)?.bytes
        ?: error("Failed to encode Compose PNG")
    File(outDir, "compose.png").writeBytes(pngBytes)
    writeBoundsJson(File(outDir, "compose_bounds.json"), widthDp, heightDp, density, tracker)
    scene.close()
    println("Compose headless render complete: ${tracker.entries.size} tracked nodes -> ${outDir.absolutePath}")
}

private fun densityDirName(label: String): String = "d" + label.replace('.', '_')

private fun runBatchHeadless(
    baseOut: File,
    widthDp: Int,
    heightDp: Int,
    densitiesArg: String?,
    defaultDensity: Float,
    stressJsonPath: File?,
    roboto: FontFamily,
): Int {
    if (densitiesArg != null) {
        val densities = densitiesArg.split(',').mapNotNull { it.trim().toFloatOrNull() }
        for (d in densities) {
            val label = formatDensity(d)
            runHeadless(File(baseOut, densityDirName(label)), widthDp, heightDp, d, label, roboto)
        }
    } else {
        runHeadless(baseOut, widthDp, heightDp, defaultDensity, formatDensity(defaultDensity), roboto)
    }
    return if (stressJsonPath != null) runComposeStressSuite(roboto, stressJsonPath) else 0
}

fun main(args: Array<String>) {
    val widthDp = argValue(args, "width")?.toIntOrNull() ?: 1760
    val heightDp = argValue(args, "height")?.toIntOrNull() ?: 980
    val density = argValue(args, "density")?.toFloatOrNull() ?: 1.0f
    val roboto = loadRoboto(resolveFontsDir(args))
    val densityLabel = formatDensity(density)
    val stressJson = argValue(args, "stress-json")?.let { File(it) }

    val headlessOut = argValue(args, "headless")
    if (headlessOut != null) {
        val code = runBatchHeadless(
            File(headlessOut),
            widthDp,
            heightDp,
            argValue(args, "densities"),
            density,
            stressJson,
            roboto,
        )
        exitProcess(code)
    }

    if (args.any { it == "--stress" }) {
        exitProcess(runComposeStressSuite(roboto, stressJson))
    }

    application {
        val tracker = BoundsTracker()
        val state = rememberWindowState(size = DpSize(widthDp.dp, (heightDp + 28).dp))
        Window(
            onCloseRequest = ::exitApplication,
            state = state,
            title = "Layout Parity Atlas - Jetpack Compose",
        ) {
            ParityAtlasScreen(
                fontFamily = roboto,
                tracker = tracker,
                viewportWidthDp = widthDp,
                viewportHeightDp = heightDp,
                densityLabel = densityLabel,
            )
        }
    }
}

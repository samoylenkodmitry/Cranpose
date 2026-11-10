package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asComposeImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import dev.perfcompare.shared.AVATAR_SIZE
import dev.perfcompare.shared.avatarArgb
import java.io.File
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ImageInfo

// What `shared-compose` asks of the desktop app, and the window `desktop.py`
// measures: 1280 x 820 points of content, the screen in `PERF_SCENARIO`
// (`gauntlet`, `wall` or `patchwork`), the tier in `PERF_TIER`, the frame to
// freeze on in `PERF_FREEZE`, the font files in `PERF_FONTS` and the pictures
// of the patchwork's chat in `PERF_CHAT`.

private val environment: Map<String, String> = System.getenv()
private val fonts = File(environment["PERF_FONTS"] ?: "fonts")

/** The folder of the animated picture and the spinner of the patchwork's chat. */
internal val chatAssets = File(environment["PERF_CHAT"] ?: "chat-assets")

/** The monospaced face of the chat's code. */
internal val Mono = FontFamily(Font(File(fonts, "DejaVuSansMono.ttf"), FontWeight.Normal))

/** The Roboto files every desktop app loads. */
internal val Roboto = FontFamily(
    Font(File(fonts, "Roboto-Regular.ttf"), FontWeight.Normal),
    Font(File(fonts, "Roboto-Bold.ttf"), FontWeight.Bold),
)

/** Writes a `PERF` line on standard output, where `desktop.py` reads it. */
fun perfLog(line: String) {
    println(line)
    System.out.flush()
}

/** Avatar [index] as an image: Skia's pixels are premultiplied BGRA, the avatar opaque ARGB. */
fun avatarBitmap(index: Int): ImageBitmap {
    val argb = avatarArgb(index)
    val bytes = ByteArray(argb.size * 4)
    for ((at, pixel) in argb.withIndex()) {
        bytes[at * 4] = pixel.toByte()
        bytes[at * 4 + 1] = (pixel shr 8).toByte()
        bytes[at * 4 + 2] = (pixel shr 16).toByte()
        bytes[at * 4 + 3] = (pixel ushr 24).toByte()
    }
    val bitmap = Bitmap()
    bitmap.allocPixels(ImageInfo.makeN32(AVATAR_SIZE, AVATAR_SIZE, ColorAlphaType.PREMUL))
    bitmap.installPixels(bytes)
    return bitmap.asComposeImageBitmap()
}

fun main() = application {
    val tier = environment["PERF_TIER"]?.toIntOrNull() ?: 5
    val freeze = environment["PERF_FREEZE"]?.toIntOrNull() ?: 0
    val scenario = environment["PERF_SCENARIO"]
    val wall = scenario == "wall"
    val patchwork = scenario == "patchwork"
    val title = if (wall) "Still wall" else if (patchwork) "Patchwork" else "Gauntlet"
    var firstFrameLogged = false
    val state = rememberWindowState(size = DpSize(1280.dp, 820.dp))
    Window(onCloseRequest = ::exitApplication, state = state, title = title, resizable = false) {
        LaunchedEffect(Unit) {
            // The window's size counts its title bar: it grows by the
            // window's insets, so the content is 1280 x 820. The insets are
            // known once the window shows its first frames.
            var insets = window.insets
            var waited = 0
            while (insets.top == 0 && waited < 120) {
                withFrameNanos { }
                insets = window.insets
                waited++
            }
            state.size = DpSize((1280 + insets.left + insets.right).dp, (820 + insets.top + insets.bottom).dp)
        }
        Column(
            Modifier
                .fillMaxSize()
                .background(if (wall || patchwork) WALL_BACKGROUND else Color(0xFFEEF0F5))
                .drawBehind {
                    if (!firstFrameLogged) {
                        firstFrameLogged = true
                        perfLog("PERF first_frame")
                    }
                },
        ) {
            TopBar(title)
            when {
                wall -> WallScreen(WallLoad(tier, freeze))
                patchwork -> PatchworkScreen(PatchworkLoad(tier, freeze))
                else -> GauntletScreen(GauntletLoad(tier, freeze))
            }
        }
    }
}

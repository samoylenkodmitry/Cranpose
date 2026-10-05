package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asComposeImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import dev.perfcompare.shared.AVATAR_SIZE
import dev.perfcompare.shared.avatarArgb
import java.awt.Dimension
import java.io.File
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ImageInfo

// What `shared-compose` asks of the desktop app, and the window `desktop.py`
// measures: 1280 x 820 points of content, the tier in `PERF_TIER`, the frame
// to freeze on in `PERF_FREEZE` and the Roboto files in `PERF_FONTS`.

private val environment: Map<String, String> = System.getenv()
private val fonts = File(environment["PERF_FONTS"] ?: "fonts")

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
    val load = GauntletLoad(
        tier = environment["PERF_TIER"]?.toIntOrNull() ?: 5,
        freeze = environment["PERF_FREEZE"]?.toIntOrNull() ?: 0,
    )
    var firstFrameLogged = false
    Window(onCloseRequest = ::exitApplication, title = "Gauntlet", resizable = false) {
        LaunchedEffect(Unit) {
            window.contentPane.preferredSize = Dimension(1280, 820)
            window.pack()
        }
        Column(
            Modifier
                .fillMaxSize()
                .background(Color(0xFFEEF0F5))
                .drawBehind {
                    if (!firstFrameLogged) {
                        firstFrameLogged = true
                        perfLog("PERF first_frame")
                    }
                },
        ) {
            TopBar("Gauntlet")
            GauntletScreen(load)
        }
    }
}

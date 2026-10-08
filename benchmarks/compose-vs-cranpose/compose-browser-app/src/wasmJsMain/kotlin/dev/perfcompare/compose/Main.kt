package dev.perfcompare.compose

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asComposeImageBitmap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import androidx.compose.ui.window.ComposeViewport
import dev.perfcompare.shared.AVATAR_SIZE
import dev.perfcompare.shared.avatarArgb
import kotlinx.browser.document
import kotlinx.browser.window
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.await
import kotlinx.coroutines.launch
import org.jetbrains.skia.Bitmap
import org.jetbrains.skia.ColorAlphaType
import org.jetbrains.skia.ImageInfo
import org.khronos.webgl.ArrayBuffer
import org.khronos.webgl.Int8Array
import org.khronos.webgl.get
import org.w3c.fetch.Response

// What `shared-compose` asks of the page: Roboto from the files beside it, the
// `PERF` lines on the console, and the avatars as Skia images. The tier and
// the freeze frame come from the query string, `?tier=12&freeze=120`.

/** The Roboto files every app loads, set before the first composition. */
internal var Roboto: FontFamily = FontFamily.Default

/** Writes a `PERF` line on the console, where the page's shim hears it. */
fun perfLog(line: String) {
    println(line)
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

/** A query parameter as a number. */
private fun query(name: String, fallback: Int): Int =
    window.location.search.trimStart('?').split('&')
        .firstOrNull { it.startsWith("$name=") }
        ?.substringAfter('=')?.toIntOrNull() ?: fallback

/** The bytes of [file], fetched from beside the page. */
private suspend fun fetchBytes(file: String): ByteArray {
    val response: Response = window.fetch(file).await()
    val buffer: ArrayBuffer = response.arrayBuffer().await()
    val array = Int8Array(buffer)
    return ByteArray(array.length) { array[it] }
}

@OptIn(ExperimentalComposeUiApi::class)
fun main() {
    MainScope().launch {
        Roboto = FontFamily(
            Font("Roboto", fetchBytes("fonts/Roboto-Regular.ttf"), FontWeight.Normal),
            Font("Roboto", fetchBytes("fonts/Roboto-Bold.ttf"), FontWeight.Bold),
        )
        val load = GauntletLoad(tier = query("tier", 5), freeze = query("freeze", 0))
        var firstFrameLogged = false
        ComposeViewport(document.body!!) {
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
}

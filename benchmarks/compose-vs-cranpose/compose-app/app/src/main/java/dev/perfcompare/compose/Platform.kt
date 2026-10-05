package dev.perfcompare.compose

import android.graphics.Bitmap
import android.util.Log
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import dev.perfcompare.shared.AVATAR_SIZE
import dev.perfcompare.shared.avatarArgb
import java.io.File

// What `shared-compose` asks of the Android app.

/** The device's Roboto faces, the same two files the Cranpose app loads. */
internal val Roboto = FontFamily(
    Font(File("/system/fonts/Roboto-Regular.ttf"), FontWeight.Normal),
    Font(File("/system/fonts/Roboto-Bold.ttf"), FontWeight.Bold),
)

/** Writes a `PERF` line to the log, under the other apps' tag. */
fun perfLog(line: String) {
    Log.i(TAG, line)
}

/** Avatar [index] as an image. */
fun avatarBitmap(index: Int): ImageBitmap =
    Bitmap.createBitmap(avatarArgb(index), AVATAR_SIZE, AVATAR_SIZE, Bitmap.Config.ARGB_8888).asImageBitmap()

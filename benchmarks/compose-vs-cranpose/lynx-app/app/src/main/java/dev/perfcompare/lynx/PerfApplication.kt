package dev.perfcompare.lynx

import android.app.Application
import android.content.Context
import android.graphics.Typeface
import android.util.Log
import com.facebook.drawee.backends.pipeline.Fresco
import com.lynx.jsbridge.LynxMethod
import com.lynx.jsbridge.LynxModule
import com.lynx.service.image.LynxImageService
import com.lynx.service.log.LynxLogService
import com.lynx.tasm.LynxEnv
import com.lynx.tasm.base.LLog
import com.lynx.tasm.behavior.shadow.text.TypefaceCache
import com.lynx.tasm.service.LynxServiceCenter

/** The log tag `measure.py` reads, as in the other apps. */
const val TAG = "PerfCompare"

/** The family name the page's text asks for: the device's Roboto files. */
const val FONT_FAMILY = "PerfRoboto"

/** What Lynx needs before a page: its image and log services, the engine, the fonts and the log module. */
class PerfApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        Fresco.initialize(this)
        LynxServiceCenter.inst().registerService(LynxImageService.getInstance())
        LynxServiceCenter.inst().registerService(LynxLogService)
        LynxEnv.inst().init(this, null, null, null)
        // Lynx logs every pipeline step at INFO; a shipped app keeps warnings.
        LLog.setMinimumLoggingLevel(LLog.WARN)
        LynxEnv.inst().registerModule("PerfLog", PerfLog::class.java)
        // The device's Roboto files, the ones the other apps load.
        TypefaceCache.cacheTypeface(FONT_FAMILY, Typeface.NORMAL, Typeface.createFromFile("/system/fonts/Roboto-Regular.ttf"))
        TypefaceCache.cacheTypeface(FONT_FAMILY, Typeface.BOLD, Typeface.createFromFile("/system/fonts/Roboto-Bold.ttf"))
    }
}

/** `NativeModules.PerfLog.line(text)`: a `PERF` line under the tag the other apps write. */
class PerfLog(context: Context) : LynxModule(context) {
    @LynxMethod
    fun line(text: String) {
        Log.i(TAG, text)
    }
}

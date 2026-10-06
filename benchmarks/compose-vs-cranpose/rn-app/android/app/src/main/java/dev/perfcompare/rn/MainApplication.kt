package dev.perfcompare.rn

import android.app.Application
import android.graphics.Typeface
import com.facebook.react.PackageList
import com.facebook.react.ReactApplication
import com.facebook.react.ReactHost
import com.facebook.react.ReactNativeApplicationEntryPoint.loadReactNative
import com.facebook.react.common.assets.ReactFontManager
import com.facebook.react.defaults.DefaultReactHost.getDefaultReactHost

class MainApplication : Application(), ReactApplication {
    override val reactHost: ReactHost by lazy {
        getDefaultReactHost(context = applicationContext, packageList = PackageList(this).packages)
    }

    override fun onCreate() {
        super.onCreate()
        // The device's Roboto files, the ones the other apps load, as the
        // `PerfRoboto` family JavaScript names.
        val fonts = ReactFontManager.getInstance()
        fonts.setTypeface(FONT_FAMILY, Typeface.NORMAL, Typeface.createFromFile("/system/fonts/Roboto-Regular.ttf"))
        fonts.setTypeface(FONT_FAMILY, Typeface.BOLD, Typeface.createFromFile("/system/fonts/Roboto-Bold.ttf"))
        loadReactNative(this)
    }

    private companion object {
        const val FONT_FAMILY = "PerfRoboto"
    }
}

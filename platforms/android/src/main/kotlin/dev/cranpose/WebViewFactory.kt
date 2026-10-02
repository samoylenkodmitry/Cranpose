package dev.cranpose

import android.content.Context
import dev.cranpose.android.CranposeWebView

/** Optional HTTPS website adapter. Enable INTERNET in the consuming application's manifest. */
class WebViewFactory : NativeViewFactory {
    override fun create(context: Context, emit: (String) -> Unit): NativeViewChild = WebChild(context, emit)
}

private class WebChild(context: Context, emit: (String) -> Unit) : NativeViewChild {
    override val view = CranposeWebView(context, emit)
    override fun update(value: String) = view.navigate(value)

    override fun setVisible(visible: Boolean) {
        super.setVisible(visible)
        if (visible) view.onResume() else view.onPause()
    }

    override fun dispose() = view.close()
}

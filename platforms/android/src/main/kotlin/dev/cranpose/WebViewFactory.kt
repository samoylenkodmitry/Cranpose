package dev.cranpose

import android.content.Context
import android.net.Uri
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient

/** Optional HTTPS website adapter. Enable INTERNET in the consuming application's manifest. */
class WebViewFactory : NativeViewFactory {
    override fun create(context: Context, emit: (String) -> Unit): NativeViewChild = WebChild(context, emit)
}

private class WebChild(context: Context, private val emit: (String) -> Unit) : NativeViewChild {
    override val view = WebView(context)
    private var source: String? = null
    private var disposed = false
    private var failed = false

    init {
        view.contentDescription = "native-webview"
        view.settings.javaScriptEnabled = true
        view.settings.domStorageEnabled = true
        view.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                if (request.url.scheme == "https") return false
                if (request.isForMainFrame) report("Unsupported WebView URL: ${request.url}")
                return true
            }
            override fun onPageStarted(view: WebView, url: String, favicon: android.graphics.Bitmap?) {
                failed = false
            }
            override fun onPageFinished(view: WebView, url: String) {
                if (!disposed && !failed) emit("loaded:$url")
            }
            override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                if (request.isForMainFrame) report(error.description.toString())
            }
            override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, error: WebResourceResponse) {
                if (request.isForMainFrame) report("HTTP ${error.statusCode}")
            }
        }
    }

    private fun report(message: String) {
        failed = true
        if (!disposed) emit("error:$message")
    }

    override fun update(value: String) {
        if (source == value) return
        source = value
        val url = Uri.parse(value)
        if (url.scheme != "https" || url.host.isNullOrEmpty()) {
            report("Unsupported WebView URL: $value")
            return
        }
        view.loadUrl(value)
    }

    override fun setVisible(visible: Boolean) {
        super.setVisible(visible)
        if (visible) view.onResume() else view.onPause()
    }

    override fun dispose() {
        if (disposed) return
        disposed = true
        view.stopLoading()
        view.destroy()
    }
}

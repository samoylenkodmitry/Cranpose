package dev.cranpose.android;

import android.content.Context;
import android.net.Uri;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import java.util.function.Consumer;

/** Website view shared by Cranpose application and embedded-component hosts. */
public final class CranposeWebView extends WebView {
    private final Consumer<String> emit;
    private String source;
    private boolean closed;
    private boolean failed;

    /** Creates a browser whose main-frame events use the Cranpose native-view protocol. */
    public CranposeWebView(Context context, Consumer<String> emit) {
        super(context);
        this.emit = emit;
        setContentDescription("native-webview");
        getSettings().setJavaScriptEnabled(true);
        getSettings().setDomStorageEnabled(true);
        setWebViewClient(new WebViewClient() {
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                if ("https".equals(request.getUrl().getScheme())) return false;
                if (request.isForMainFrame()) report("Unsupported WebView URL: " + request.getUrl());
                return true;
            }
            @Override public void onPageStarted(WebView view, String url, android.graphics.Bitmap icon) { failed = false; }
            @Override public void onPageFinished(WebView view, String url) {
                if (!closed && !failed) emit.accept("loaded:" + url);
            }
            @Override public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) report(error.getDescription().toString());
            }
            @Override public void onReceivedHttpError(WebView view, WebResourceRequest request, WebResourceResponse response) {
                if (request.isForMainFrame()) report("HTTP " + response.getStatusCode());
            }
        });
    }

    private void report(String message) {
        failed = true;
        if (!closed) emit.accept("error:" + message);
    }

    /** Loads a changed HTTPS URL, preserving the current document otherwise. */
    public void navigate(String url) {
        if (closed || url.equals(source)) return;
        source = url;
        Uri parsed = Uri.parse(url);
        if (!"https".equals(parsed.getScheme()) || parsed.getHost() == null || parsed.getHost().isEmpty()) {
            report("Unsupported WebView URL: " + url);
            return;
        }
        loadUrl(url);
    }

    /** Stops loading and releases the browser once it has been removed from its parent. */
    public void close() {
        if (closed) return;
        closed = true;
        stopLoading();
        destroy();
    }
}

import UIKit
import WebKit

/// Optional HTTPS website factory. Navigation is handled by the platform WebView.
@MainActor public struct WebViewFactory: NativeViewFactory {
    public init() {}
    public func create(emit: @escaping (String) -> Void) -> any NativeViewChild { WebChild(emit: emit) }
}

@MainActor private final class WebChild: NSObject, NativeViewChild, WKNavigationDelegate {
    private let web = WKWebView()
    var view: UIView { web }
    private var source: String?
    private var disposed = false
    private let emit: (String) -> Void

    init(emit: @escaping (String) -> Void) {
        self.emit = emit
        super.init()
        web.navigationDelegate = self
        web.accessibilityIdentifier = "native-webview"
    }

    func update(_ value: String) {
        guard source != value else { return }
        source = value
        guard let url = URL(string: value), url.scheme == "https", let host = url.host, !host.isEmpty else {
            report("Unsupported WebView URL: \(value)")
            return
        }
        web.load(URLRequest(url: url))
    }

    func dispose() {
        guard !disposed else { return }
        disposed = true
        web.stopLoading()
        web.navigationDelegate = nil
        web.removeFromSuperview()
    }

    private func report(_ message: String) {
        if !disposed { emit("error:\(message)") }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        if !disposed, let url = webView.url { emit("loaded:\(url.absoluteString)") }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        if (error as NSError).code != NSURLErrorCancelled { report(error.localizedDescription) }
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        self.webView(webView, didFail: navigation, withError: error)
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if action.request.url?.scheme == "https" { decisionHandler(.allow) }
        else {
            if action.targetFrame?.isMainFrame != false {
                report("Unsupported WebView URL: \(action.request.url?.absoluteString ?? "")")
            }
            decisionHandler(.cancel)
        }
    }

    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse,
                 decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        if response.isForMainFrame, let http = response.response as? HTTPURLResponse, http.statusCode >= 400 {
            report("HTTP \(http.statusCode)")
            decisionHandler(.cancel)
        } else { decisionHandler(.allow) }
    }
}

import UIKit
import WebKit

@MainActor private final class FrameRelay: FrameListener {
    weak var view: CranposeView?
    nonisolated func requestFrame() {
        Task { @MainActor [weak self] in self?.view?.requestFrame() }
    }
}

final class CranposeView: UIView {
    var onCount: ((Int32) -> Void)?
    var onError: ((String) -> Void)?
    private let image = UIImageView()
    private let relay = FrameRelay()
    private var session: DemoSession!
    private var inFlight = false
    private var pending = false
    private var scheduled: DispatchWorkItem?
    private var closed = false
    private var nativeChildren: [UInt64: WebChild] = [:]
    private var primaryTouch: UITouch?

    init(nativeChildren: Bool) {
        super.init(frame: .zero)
        clipsToBounds = true
        accessibilityIdentifier = "cranpose-component"
        backgroundColor = .secondarySystemBackground
        image.isUserInteractionEnabled = false
        addSubview(image)
        relay.view = self
        session = DemoSession(nativeChildren: nativeChildren, listener: relay)
        NotificationCenter.default.addObserver(self, selector: #selector(pause),
            name: UIApplication.didEnterBackgroundNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(resume),
            name: UIApplication.didBecomeActiveNotification, object: nil)
    }

    required init?(coder: NSCoder) { fatalError("Use init(nativeChildren:)") }

    deinit {
        NotificationCenter.default.removeObserver(self)
        session?.shutdown()
    }

    func close() {
        closed = true
        scheduled?.cancel()
        scheduled = nil
        session.shutdown()
        NotificationCenter.default.removeObserver(self)
        for child in nativeChildren.values { child.dispose() }
        nativeChildren.removeAll()
    }

    func increment() {
        do { try session.increment(); requestFrame() }
        catch { onError?(error.localizedDescription) }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        image.frame = bounds
        requestFrame()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { pause() } else { resume() }
    }

    @objc private func pause() {
        guard !closed else { return }
        do { try session.setVisible(visible: false) }
        catch { onError?(error.localizedDescription) }
        scheduled?.cancel()
        scheduled = nil
        primaryTouch = nil
    }

    @objc private func resume() {
        guard !closed, window != nil else { return }
        do { try session.setVisible(visible: true) }
        catch { onError?(error.localizedDescription) }
        requestFrame()
    }

    func requestFrame() {
        guard !closed, window != nil, UIApplication.shared.applicationState != .background,
              bounds.width > 0, bounds.height > 0 else { return }
        pending = true
        guard !inFlight, scheduled == nil else { return }
        let work = DispatchWorkItem { [weak self] in
            self?.scheduled = nil
            self?.render()
        }
        scheduled = work
        DispatchQueue.main.async(execute: work)
    }

    private func render() {
        guard !closed, window != nil, UIApplication.shared.applicationState != .background, !inFlight else { return }
        pending = false
        inFlight = true
        let scale = window?.screen.scale ?? 1
        let size = bounds.size
        let width = UInt32(max(1, (size.width * scale).rounded()))
        let height = UInt32(max(1, (size.height * scale).rounded()))
        Task { [weak self, session] in
            do {
                let frame = try await session!.frame(width: width, height: height, density: Float(scale))
                guard let self, !self.closed else { return }
                self.inFlight = false
                if !frame.pixels.isEmpty {
                    try self.present(frame, scale: scale)
                    self.reconcile(frame.slots)
                }
                self.onCount?(frame.count)
                if self.pending { self.requestFrame() }
                else if let delay = frame.nextFrameMs, self.window != nil {
                    let work = DispatchWorkItem { [weak self] in
                        self?.scheduled = nil
                        self?.requestFrame()
                    }
                    self.scheduled = work
                    DispatchQueue.main.asyncAfter(deadline: .now() + Double(delay) / 1000, execute: work)
                }
            } catch {
                guard let self, !self.closed else { return }
                self.inFlight = false
                self.onError?(error.localizedDescription)
            }
        }
    }

    private func present(_ frame: Frame, scale: CGFloat) throws {
        guard let provider = CGDataProvider(data: frame.pixels as CFData),
              let cgImage = CGImage(
                width: Int(frame.width), height: Int(frame.height),
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(frame.width) * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
                    .union(.byteOrder32Big),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { throw NSError(domain: "Cranpose", code: 1,
            userInfo: [NSLocalizedDescriptionKey: "Invalid rendered image"]) }
        image.image = UIImage(cgImage: cgImage, scale: scale, orientation: .up)
    }

    private func reconcile(_ slots: [NativeSlot]) {
        let ids = Set(slots.map(\.id))
        for id in Array(nativeChildren.keys) where !ids.contains(id) {
            nativeChildren.removeValue(forKey: id)?.dispose()
        }
        for slot in slots {
            guard slot.kind == "web" else { continue }
            let child: WebChild
            if let existing = nativeChildren[slot.id] { child = existing }
            else {
                child = WebChild { [weak self] in
                    guard let self, !self.closed else { return }
                    do {
                        try self.session.nativeEvent(id: slot.id, event: "increment")
                        self.requestFrame()
                    } catch { self.onError?(error.localizedDescription) }
                }
                nativeChildren[slot.id] = child
                addSubview(child.view)
            }
            child.view.frame = CGRect(x: CGFloat(slot.x), y: CGFloat(slot.y),
                width: CGFloat(slot.width), height: CGFloat(slot.height))
            child.update(slot.value)
            bringSubviewToFront(child.view)
        }
    }

    private func send(_ phase: TouchPhase, _ touch: UITouch) {
        let point = touch.location(in: self)
        do {
            try session.touch(phase: phase, x: Float(point.x), y: Float(point.y))
            requestFrame()
        } catch { onError?(error.localizedDescription) }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        if primaryTouch == nil, let touch = touches.first {
            primaryTouch = touch
            send(.down, touch)
        }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch, touches.contains(touch) { send(.move, touch) }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch, touches.contains(touch) {
            send(.up, touch)
            primaryTouch = nil
        }
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch { send(.cancel, touch); primaryTouch = nil }
    }
}

private final class WebChild: NSObject, WKNavigationDelegate {
    let view = WKWebView()
    private var html = ""
    private let onIncrement: () -> Void

    init(onIncrement: @escaping () -> Void) {
        self.onIncrement = onIncrement
        super.init()
        view.navigationDelegate = self
        view.accessibilityIdentifier = "native-webview"
    }

    func update(_ value: String) {
        guard html != value else { return }
        html = value
        view.loadHTMLString(value, baseURL: nil)
    }

    func dispose() {
        view.stopLoading()
        view.navigationDelegate = nil
        view.removeFromSuperview()
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        if navigationAction.request.url?.absoluteString == "cranpose:increment" {
            onIncrement()
            decisionHandler(.cancel)
        } else {
            decisionHandler(navigationAction.navigationType == .linkActivated ? .cancel : .allow)
        }
    }
}

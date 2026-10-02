import UIKit
import CranposeBindings

@MainActor private final class FrameRelay: NSObject, FrameListener {
    weak var view: CranposeView?
    nonisolated func requestFrame() {
        Task { @MainActor [weak self] in self?.view?.requestFrame() }
    }
    @objc func displayFrame() { view?.displayFrame() }
}

/// A native view that owns a Cranpose session, rendering, input and native children.
@MainActor public final class CranposeView: UIView {
    /// Application events emitted by the Rust composition.
    public var onEvent: ((NativeEvent) -> Void)?
    /// Rendering, native factory and transport failures.
    public var onError: ((String) -> Void)?
    private let session: NativeSession
    private let factories: [String: any NativeViewFactory]
    private let image = UIImageView()
    private let relay = FrameRelay()
    private var displayLink: CADisplayLink?
    private var timer: DispatchWorkItem?
    private var inFlight = false
    private var pending = false
    private var closed = false
    private var contentVisible = false
    private var primaryTouch: UITouch?
    private lazy var registry = NativeViewRegistry(parent: self, factories: factories) { [weak self] id, event in
        self?.command { try self?.session.nativeEvent(id: id, event: event) }
    }

    /// Inserts a session into UIKit. Register optional or custom native factories by kind.
    public init(session: NativeSession, factories: [String: any NativeViewFactory] = [:]) {
        self.session = session
        self.factories = factories
        super.init(frame: .zero)
        clipsToBounds = true
        accessibilityIdentifier = "cranpose-component"
        image.isUserInteractionEnabled = false
        addSubview(image)
        relay.view = self
        let link = CADisplayLink(target: relay, selector: #selector(FrameRelay.displayFrame))
        link.isPaused = true
        link.add(to: .main, forMode: .common)
        displayLink = link
        command { try session.setListener(listener: relay) }
        NotificationCenter.default.addObserver(self, selector: #selector(visibilityChanged),
            name: UIApplication.didEnterBackgroundNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(visibilityChanged),
            name: UIApplication.didBecomeActiveNotification, object: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(session:factories:)") }

    deinit {
        displayLink?.invalidate()
        timer?.cancel()
        NotificationCenter.default.removeObserver(self)
        session.shutdown()
    }

    /// Sends application input to Rust.
    public func sendEvent(_ name: String, value: String = "") {
        command { try session.sendEvent(name: name, value: value) }
    }

    /// Releases this view's session and every native child. Idempotent.
    public func close() {
        guard !closed else { return }
        closed = true
        timer?.cancel()
        timer = nil
        displayLink?.invalidate()
        displayLink = nil
        session.shutdown()
        registry.close()
        image.image = nil
        NotificationCenter.default.removeObserver(self)
    }

    public override var isHidden: Bool { didSet { visibilityChanged() } }
    public override var alpha: CGFloat { didSet { visibilityChanged() } }
    public override func didMoveToWindow() { super.didMoveToWindow(); visibilityChanged() }
    public override func didMoveToSuperview() { super.didMoveToSuperview(); visibilityChanged() }
    public override func layoutSubviews() { super.layoutSubviews(); image.frame = bounds; requestFrame() }

    private var visible: Bool {
        guard window != nil, UIApplication.shared.applicationState != .background else { return false }
        var ancestor: UIView? = self
        while let view = ancestor {
            if view.isHidden || view.alpha == 0 { return false }
            ancestor = view.superview
        }
        return true
    }

    @objc private func visibilityChanged() {
        updateVisibility()
        if contentVisible { requestFrame() }
    }

    private func updateVisibility() {
        guard !closed else { return }
        let next = visible
        guard next != contentVisible else { return }
        contentVisible = next
        registry.setVisible(next)
        command { try session.setVisible(visible: next) }
        if !next {
            timer?.cancel()
            timer = nil
            displayLink?.isPaused = true
            primaryTouch = nil
        }
    }

    fileprivate func requestFrame() {
        updateVisibility()
        guard !closed, contentVisible, bounds.width > 0, bounds.height > 0 else { return }
        pending = true
        timer?.cancel()
        timer = nil
        if !inFlight { displayLink?.isPaused = false }
    }

    fileprivate func displayFrame() {
        displayLink?.isPaused = true
        updateVisibility()
        guard !closed, contentVisible, !inFlight else { return }
        pending = false
        inFlight = true
        let scale = window?.screen.scale ?? 1
        let width = UInt32(max(1, (bounds.width * scale).rounded()))
        let height = UInt32(max(1, (bounds.height * scale).rounded()))
        Task { [weak self, session] in
            do {
                let frame = try await session.frame(width: width, height: height, density: Float(scale))
                guard let self, !self.closed else { return }
                self.inFlight = false
                if !frame.pixels.isEmpty {
                    try self.present(frame, scale: scale)
                    try self.registry.reconcile(frame.slots)
                }
                for event in frame.events { self.onEvent?(event) }
                guard !self.closed, self.contentVisible else { return }
                if self.pending { self.requestFrame() }
                else if let delay = frame.nextFrameMs {
                    if delay == 0 { self.requestFrame() }
                    else {
                        let work = DispatchWorkItem { [weak self] in self?.requestFrame() }
                        self.timer = work
                        DispatchQueue.main.asyncAfter(deadline: .now() + Double(delay) / 1000, execute: work)
                    }
                }
            } catch {
                guard let self, !self.closed else { return }
                self.inFlight = false
                self.onError?(error.localizedDescription)
            }
        }
    }

    private func present(_ frame: NativeFrame, scale: CGFloat) throws {
        guard let provider = CGDataProvider(data: frame.pixels as CFData),
              let cgImage = CGImage(width: Int(frame.width), height: Int(frame.height),
                bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(frame.width) * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue).union(.byteOrder32Big),
                provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
        else { throw NSError(domain: "Cranpose", code: 2,
            userInfo: [NSLocalizedDescriptionKey: "Invalid rendered image"]) }
        image.image = UIImage(cgImage: cgImage, scale: scale, orientation: .up)
    }

    private func command(_ action: () throws -> Void) {
        guard !closed else { return }
        do { try action() } catch { onError?(error.localizedDescription) }
    }

    private func send(_ phase: TouchPhase, _ touch: UITouch) {
        let point = touch.location(in: self)
        command { try session.touch(phase: phase, x: Float(point.x), y: Float(point.y)) }
    }

    public override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        if primaryTouch == nil, let touch = touches.first { primaryTouch = touch; send(.down, touch) }
    }
    public override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch, touches.contains(touch) { send(.move, touch) }
    }
    public override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch, touches.contains(touch) { send(.up, touch); primaryTouch = nil }
    }
    public override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        if let touch = primaryTouch { send(.cancel, touch); primaryTouch = nil }
    }
}

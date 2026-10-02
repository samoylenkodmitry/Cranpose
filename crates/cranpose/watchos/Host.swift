import SwiftUI
import WatchKit

@MainActor
final class CranposeModel: ObservableObject {
    @Published var image: CGImage?
    @Published var error: String?
    private var timer: Timer?
    private let epoch = ProcessInfo.processInfo.systemUptime
    private var ready = false

    var elapsed: UInt64 {
        UInt64(max(0, ProcessInfo.processInfo.systemUptime - epoch) * 1_000_000_000)
    }

    func resize(_ size: CGSize) {
        let scale = WKInterfaceDevice.current().screenScale
        let width = UInt32(max(1, (size.width * scale).rounded()))
        let height = UInt32(max(1, (size.height * scale).rounded()))
        if !CPResize(width, height, Float(scale)) {
            error = String(cString: CPError())
            return
        }
        ready = true
        tick()
    }

    func tick() {
        guard ready else { return }
        if let frame = CPFrame(elapsed) { image = frame }
    }

    func activate(_ active: Bool) {
        CPActive(active)
        timer?.invalidate()
        timer = nil
        if active {
            timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
                MainActor.assumeIsolated { self?.tick() }
            }
            tick()
        }
    }
}

struct CranposeView: View {
    @StateObject private var model = CranposeModel()
    @Environment(\.scenePhase) private var phase
    @State private var crown = 0.0
    @State private var touching = false

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                Color.black
                if let image = model.image {
                    Image(decorative: image, scale: WKInterfaceDevice.current().screenScale)
                        .resizable().interpolation(.none).allowsHitTesting(false)
                }
                if let error = model.error { Text(error).foregroundStyle(.red) }
            }
            .contentShape(Rectangle())
            .focusable()
            .digitalCrownRotation($crown, from: -1_000_000, through: 1_000_000, by: 1,
                                  sensitivity: .medium, isContinuous: true, isHapticFeedbackEnabled: false)
            .onChange(of: crown) { old, value in CPCrown(Float(value - old), model.elapsed / 1_000_000) }
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { event in
                    if !touching {
                        CPTouch(0, Float(event.startLocation.x), Float(event.startLocation.y))
                        touching = true
                    }
                    CPTouch(1, Float(event.location.x), Float(event.location.y))
                }
                .onEnded { event in
                    CPTouch(2, Float(event.location.x), Float(event.location.y))
                    touching = false
                })
            .onAppear { model.resize(geometry.size); model.activate(phase == .active) }
            .onChange(of: geometry.size) { _, size in model.resize(size) }
            .onChange(of: phase) { _, phase in touching = false; model.activate(phase == .active) }
            .onDisappear { touching = false; model.activate(false) }
        }
        .ignoresSafeArea()
    }
}

@main
struct CranposeWatchApp: App {
    var body: some Scene { WindowGroup { CranposeView() } }
}

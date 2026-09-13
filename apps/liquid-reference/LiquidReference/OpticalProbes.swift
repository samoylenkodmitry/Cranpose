import SwiftUI
import QuartzCore

struct OpticalPattern: Codable {
    let kind: String
    let axis: String
    let period: Double
    let phase: Int
    let level: Double
    var channel: String = "all"
    var isolatedLayer: String = "all"
    var sourceLayer: String = "none"
    var contactActivity: Double? = nil

    func drawStrips(size: CGSize, origin: CGPoint, scale: CGFloat, draw: (CGRect, Color) -> Void) {
        if kind == "flat" {
            draw(CGRect(origin: .zero, size: size), Color(.sRGB, white: level))
            return
        }
        if kind == "step" {
            draw(CGRect(origin: .zero, size: size), .black)
            let horizontal = axis == "x"
            let boundary = (horizontal ? 334.0 : 822.0) + Double(phase) / (4 * scale)
            let rect = horizontal
                ? CGRect(x: boundary, y: 0, width: size.width - boundary, height: size.height)
                : CGRect(x: 0, y: boundary, width: size.width, height: size.height - boundary)
            draw(rect, .white)
            return
        }
        let horizontal = axis == "x"
        let extent = horizontal ? size.width : size.height
        let start = horizontal ? origin.x : origin.y
        for pixel in 0..<Int(ceil(extent * scale)) {
            let coordinate = (CGFloat(pixel) + 0.5) / scale + start
            let angle = 2 * Double.pi * coordinate / period + Double(phase) * Double.pi / 2
            let level = 0.5 + 0.15 * cos(angle)
            let rect = horizontal
                ? CGRect(x: CGFloat(pixel) / scale, y: 0, width: 1 / scale, height: size.height)
                : CGRect(x: 0, y: CGFloat(pixel) / scale, width: size.width, height: 1 / scale)
            draw(rect, Color(.sRGB,
                             red: channel == "all" || channel == "red" ? level : 0.5,
                             green: channel == "all" || channel == "green" ? level : 0.5,
                             blue: channel == "all" || channel == "blue" ? level : 0.5))
        }
    }

    static let sequence: [OpticalPattern] = {
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "resting-blur" {
            return (0..<4).map { phase in
                OpticalPattern(kind: "step", axis: "x", period: 0, phase: phase, level: 0.5, sourceLayer: "restingBlurKernel")
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "background-opacity" {
            return ["x", "y"].flatMap { axis in
                [0.0, 0.5, 1.0].map { opacity in
                    OpticalPattern(kind: "wave", axis: axis, period: 32, phase: 0, level: 0.5,
                                   sourceLayer: "backgroundKernel", contactActivity: opacity)
                }
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "inner-shadow" {
            return (0..<2).map { phase in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: phase, level: 0.35, sourceLayer: "innerShadowMask")
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "pane-image" {
            return [0.35, 0.65].map { level in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: level, isolatedLayer: "raisedLens")
            } + ["x", "y"].flatMap { axis in
                (0..<4).map { phase in
                    OpticalPattern(kind: "wave", axis: axis, period: 32, phase: phase, level: 0.5, isolatedLayer: "raisedLens")
                }
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "lens-shadow" {
            return ["all", "lensShadow"].map { layer in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: 0.35, isolatedLayer: layer)
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "glow-mask" {
            return (0..<2).map { phase in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: phase, level: 0.35, sourceLayer: "localGlowMask")
            }
        }
        if let kernel = ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"],
           ["adaptive-boundaries", "adaptive-boundaries-fine"].contains(kernel) {
            let levels = kernel == "adaptive-boundaries"
                ? [0.18, 0.20, 0.22, 0.24, 0.90, 0.94, 0.96, 0.98]
                : [0.25, 0.26, 0.27, 0.28, 0.86, 0.87, 0.88, 0.89]
            return levels.map { level in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: level)
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "adaptive-flat" {
            return [0.0, 0.15, 0.3, 0.5, 0.7, 0.85, 1.0].map { level in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: level)
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"] == "contact-step" {
            return [0.0, 0.25, 0.5, 0.75, 1.0].flatMap { activity in
                ["x", "y"].map { axis in
                    OpticalPattern(kind: "step", axis: axis, period: 0, phase: 0, level: 0.5,
                                   sourceLayer: "chromaticKernel", contactActivity: activity)
                }
            }
        }
        if let kernel = ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_KERNEL"],
           ["foreground-wave", "foreground-step", "background-wave", "pane-wave", "content-wave"].contains(kernel) {
            return ["x", "y"].flatMap { axis in
                (0..<4).map { phase in
                    OpticalPattern(kind: kernel == "foreground-step" ? "step" : "wave", axis: axis,
                                   period: kernel == "pane-wave" ? Double(ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_PERIOD"] ?? "128") ?? 128 : 32, phase: phase, level: 0.5,
                                   sourceLayer: kernel == "content-wave" ? "contentKernel" : kernel == "pane-wave" ? "paneKernel" : kernel == "background-wave" ? "backgroundKernel" : "chromaticKernel")
                }
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_MONOCHROME"] == "1" {
            return ["x", "y"].flatMap { axis in
                (0..<4).map { phase in
                    OpticalPattern(kind: "wave", axis: axis, period: 32, phase: phase, level: 0.5, isolatedLayer: "chromaticForeground")
                }
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_SDF"] == "1" {
            return ["initialWarp", "liftedWarp", "contentWarp", "highlight", "backgroundField", "foregroundField", "paneHighlight", "paneHighlightTall", "highlightTall"].flatMap { layer in
                (0..<2).map { phase in
                    OpticalPattern(kind: "flat", axis: "", period: 0, phase: phase, level: 0.35, sourceLayer: layer)
                }
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_LAYERS"] == "1" {
            return ["all", "initialWarp", "liftedWarp", "contentWarp", "chromaticForeground", "highlight", "globalGlow", "localGlow", "lensBackgroundWarp", "paneHighlight", "paneHighlightAndGlobal", "innerShadow"].map { layer in
                OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: 0.35, isolatedLayer: layer)
            }
        }
        if ProcessInfo.processInfo.environment["REFERENCE_OPTICAL_CHANNELS"] == "1" {
            return ["red", "green", "blue"].flatMap { channel in
                ["x", "y"].flatMap { axis in
                    (0..<4).map { phase in
                        OpticalPattern(kind: "wave", axis: axis, period: 32, phase: phase, level: 0.5, channel: channel)
                    }
                }
            }
        }
        var result = [OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: 0.35),
                      OpticalPattern(kind: "flat", axis: "", period: 0, phase: 0, level: 0.65)]
        for (axis, periods) in [("x", [512.0, 128, 32, 8]), ("y", [1024.0, 128, 32, 8])] {
            for period in periods {
                for phase in 0..<4 {
                    result.append(OpticalPattern(kind: "wave", axis: axis, period: period, phase: phase, level: 0.5))
                }
            }
        }
        result.append(OpticalPattern(kind: "checkerboard", axis: "", period: 8, phase: 0, level: 0.5))
        return result
    }()
}

struct OpticalFrame: Codable {
    let run: String
    let index: Int
    let count: Int
    let changedWallMillis: Double
    let duration: Double
    let pattern: OpticalPattern
    let viewport: [CGFloat]
    let layers: [LayerSample]
}

@MainActor
@Observable
final class OpticalProbes {
    let enabled: Bool
    private(set) var index: Int
    private(set) var complete = false
    @ObservationIgnored private var began: TimeInterval?
    @ObservationIgnored private var run = ""

    init() {
        let environment = ProcessInfo.processInfo.environment
        enabled = environment["REFERENCE_OPTICAL_PROBES"] == "1"
        index = enabled ? min(max(Int(environment["REFERENCE_OPTICAL_INDEX"] ?? "0") ?? 0, 0), OpticalPattern.sequence.count - 1) : -1
    }

    var running: Bool { began != nil && !complete }
    var pattern: OpticalPattern? { OpticalPattern.sequence.indices.contains(index) ? OpticalPattern.sequence[index] : nil }
    var label: String { "Optical probe \(index + 1)/\(OpticalPattern.sequence.count)" + (complete ? " captured" : " ready") }

    func next() {
        guard enabled, !running, index + 1 < OpticalPattern.sequence.count else { return }
        index += 1
        complete = false
        began = nil
    }

    func start() {
        guard enabled else { return }
        began = CACurrentMediaTime() + 1.75
        run = UUID().uuidString
        complete = false
    }

    func advance(now: TimeInterval, viewport: CGSize, layers: () -> [LayerSample]) throws {
        guard let began, now >= began, !complete else { return }
        let frame = OpticalFrame(run: run, index: index, count: OpticalPattern.sequence.count,
                                 changedWallMillis: Date().timeIntervalSince1970 * 1000,
                                 duration: 2, pattern: OpticalPattern.sequence[index],
                                 viewport: [viewport.width, viewport.height], layers: layers())
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.nonConformingFloatEncodingStrategy = .convertToString(positiveInfinity: "+Infinity", negativeInfinity: "-Infinity", nan: "NaN")
        let data = try encoder.encode(frame)
        try data.write(to: URL.documentsDirectory.appending(path: "optical-probe-\(index).json"), options: .atomic)
        try data.write(to: URL.documentsDirectory.appending(path: "optical-probe.json"), options: .atomic)
        complete = true
    }
}

struct OpticalBackdrop: View {
    let pattern: OpticalPattern
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        GeometryReader { geometry in
            let origin = geometry.frame(in: .global).origin
            Canvas { context, size in
                pattern.drawStrips(size: size, origin: origin, scale: displayScale) { rect, color in
                    context.fill(Path(rect), with: .color(color))
                }
            }
        }
        .accessibilityHidden(true)
    }
}

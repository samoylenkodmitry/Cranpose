import SwiftUI
import UIKit
import QuartzCore
import CoreText

@MainActor
private func requestDisplayRefresh(_ link: CADisplayLink, screen: UIScreen) {
    let maximum = Float(screen.maximumFramesPerSecond)
    link.preferredFrameRateRange = CAFrameRateRange(minimum: maximum, maximum: maximum, preferred: maximum)
}

struct LayerSample: Codable {
    let path: String
    let kind: String
    let rect: [CGFloat]?
    let opacity: Float
    let cornerRadius: CGFloat
    var filterValues: [[String: String]]? = nil
    var descriptor: String? = nil
    var archive: String? = nil
    var effectArchive: String? = nil
    var bounds: [CGFloat]? = nil
    var scale: [CGFloat]? = nil
    var animations: [[String: String]]? = nil
}

struct MotionSample: Codable {
    let timestamp: TimeInterval
    let targetTimestamp: TimeInterval
    let phase: String
    let layers: [LayerSample]
}

struct TouchSample: Codable {
    let wallMillis: Double
    let receivedWallMillis: Double
    let timestamp: TimeInterval
    let phase: String
    let x: CGFloat
    let y: CGFloat
}

@MainActor
@Observable
final class NativeTrace: NSObject {
    let optics = OpticalProbes()
    var phase = "Ready · touch the floating bar"
    var speed = 0.0
    var frameCount = 0
    var error: String?
    @ObservationIgnored private weak var window: UIWindow?
    @ObservationIgnored private var displayLink: CADisplayLink?
    @ObservationIgnored private var touches: [TouchSample] = []
    @ObservationIgnored private var frames: [MotionSample] = []
    @ObservationIgnored private var held = false
    @ObservationIgnored private var moved = false
    @ObservationIgnored private var lastMove = 0.0
    @ObservationIgnored private var began = 0.0
    @ObservationIgnored private var released = 0.0
    @ObservationIgnored private var saved = false
    @ObservationIgnored private var configuredProbe: Int?
    @ObservationIgnored private var recordingFilters = false
    @ObservationIgnored private var pendingContactFilters: [(Int, [Any])] = []
    @ObservationIgnored private var pendingContactEffects: [(Int, Any)] = []
    @ObservationIgnored private var contactFiltersSaved = false
    @ObservationIgnored private let contactFilterTime = ProcessInfo.processInfo.environment["REFERENCE_CONTACT_FILTER_TIME"].flatMap(Double.init)
    @ObservationIgnored private let settlingSeconds = ProcessInfo.processInfo.environment["REFERENCE_SETTLING_SECONDS"].flatMap(Double.init) ?? 0.8

    func install(on window: UIWindow) {
        guard self.window !== window else { return }
        displayLink?.invalidate()
        self.window = window
        let observer = PassiveTouchObserver(trace: self)
        window.addGestureRecognizer(observer)
        let link = CADisplayLink(target: self, selector: #selector(tick))
        requestDisplayRefresh(link, screen: window.screen)
        link.add(to: .main, forMode: .common)
        link.isPaused = true
        displayLink = link
    }

    func receive(_ touch: UITouch, phase: String) {
        let receivedWallMillis = Date().timeIntervalSince1970 * 1000
        let eventWallMillis = receivedWallMillis + (touch.timestamp - CACurrentMediaTime()) * 1000
        guard let window else { return }
        let point = touch.location(in: window)
        if phase == "Touch down" {
            guard point.y > window.bounds.height - 120 else { return }
            if ProcessInfo.processInfo.environment["REFERENCE_EXPORT_CONTENT"] == "1", let bar = findTabBar(in: window) {
                do { try exportContent(bar) } catch { self.error = String(describing: error) }
            }
            held = true
            if point.x > window.bounds.width * 0.7 { optics.start() }
            displayLink?.isPaused = false
            error = nil
            moved = false
            began = touch.timestamp
            lastMove = touch.timestamp
            speed = 0
            touches = []
            frames = []
            saved = false
        }
        guard held else { return }
        if phase == "Sliding", let previous = touches.last {
            let dt = touch.timestamp - previous.timestamp
            if dt > 0 {
                speed = hypot(point.x - previous.x, point.y - previous.y) / dt
            }
            moved = true
            lastMove = touch.timestamp
        }
        touches.append(TouchSample(wallMillis: eventWallMillis, receivedWallMillis: receivedWallMillis,
                                   timestamp: touch.timestamp, phase: phase, x: point.x, y: point.y))
        self.phase = phase
        if phase == "Touch up" || phase == "Cancelled" {
            held = false
            released = CACurrentMediaTime()
        }
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let window else { return }
        let now = CACurrentMediaTime()
        do {
            if !touches.isEmpty, !saved, let contactFilterTime, now - began >= contactFilterTime, !contactFiltersSaved {
                try recordContactFilters(in: window, elapsed: now - began)
                contactFiltersSaved = true
            }
            if held && optics.enabled && now - began > 1.5 && configuredProbe != optics.index {
                try isolateOpticalLayer(in: window)
                configuredProbe = optics.index
            }
            try optics.advance(now: now, viewport: window.bounds.size) { sampleTabBar(in: window) }
        } catch { self.error = String(describing: error) }
        guard !touches.isEmpty, !saved else {
            if !optics.running { link.isPaused = true }
            return
        }
        if held && now - began > 0.08 {
            let idle = now - lastMove
            phase = idle > 0.2 ? "Idle · finger held" : moved && idle > 0.06 ? "Stopped" : moved ? "Sliding" : "Pressed"
            if idle > 0.06 { speed = 0 }
        }
        if !optics.enabled {
            let layers = sampleTabBar(in: window)
            frames.append(MotionSample(timestamp: link.timestamp, targetTimestamp: link.targetTimestamp,
                                       phase: phase, layers: layers))
            if frames.count.isMultiple(of: 12) { frameCount = frames.count }
        }
        if !held && now - released >= settlingSeconds {
            saved = true
            link.isPaused = !optics.running
            frameCount = frames.count
            do {
                let encoder = JSONEncoder()
                encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
                encoder.nonConformingFloatEncodingStrategy = .convertToString(positiveInfinity: "+Infinity", negativeInfinity: "-Infinity", nan: "NaN")
                let name = "native-\(Int(touches[0].wallMillis))"
                try encoder.encode(touches).write(to: URL.documentsDirectory.appending(path: name + "-touches.json"), options: .atomic)
                try encoder.encode(frames).write(to: URL.documentsDirectory.appending(path: name + "-layers.json"), options: .atomic)
                phase = "Settled · trace saved"
            } catch {
                phase = "Trace could not be saved"
                self.error = String(describing: error)
            }
        }
    }

    private enum OpticalLayerError: Error {
        case missingLayer(String)
        case missingFilter(String)
        case unknownProbe(String)
    }

    private func isolateOpticalLayer(in window: UIWindow) throws {
        if let pattern = optics.pattern, pattern.sourceLayer != "none" {
            try exposeOpticalSDF(pattern, in: window)
            return
        }
        guard let name = optics.pattern?.isolatedLayer, name != "all" else { return }
        let path: [Int]
        let filterName: String?
        let parameter: String
        switch name {
        case "raisedLens": (path, filterName, parameter) = ([0, 0, 1], nil, "")
        case "initialWarp": (path, filterName, parameter) = ([0, 0, 1, 0, 0], "displacementMap", "inputAmount")
        case "liftedWarp": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 0, 0], "displacementMap", "inputAmount")
        case "contentWarp": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 1, 0, 0, 1], "displacementMap", "inputAmount")
        case "lensBackgroundWarp": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 1, 0, 0, 0], "glassBackground", "inputInnerRefractionAmount")
        case "lensShadow": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 1, 0, 0, 0], "glassBackground", "inputShadowOpacity")
        case "chromaticForeground": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 1, 0, 0, 2], "glassForeground", "inputAberrationAmount")
        case "highlight": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 1, 0, 0, 3], nil, "")
        case "globalGlow": (path, filterName, parameter) = ([0, 1, 0], nil, "")
        case "innerShadow": (path, filterName, parameter) = ([0, 0, 1, 0, 2, 0, 1], nil, "")
        case "localGlow": (path, filterName, parameter) = ([0, 1, 1], nil, "")
        case "paneHighlight", "paneHighlightAndGlobal": (path, filterName, parameter) = ([0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1], nil, "")
        default: throw OpticalLayerError.unknownProbe(name)
        }
        let layer = try opticalLayer(path, named: name, in: window, fromWindow: name.hasPrefix("paneHighlight")).model()
        if name == "raisedLens" {
            guard let view = layer.delegate as? UIView else { throw OpticalLayerError.missingLayer(name) }
            view.removeFromSuperview()
            return
        }
        if name == "innerShadow" {
            layer.shadowOpacity = 0
            return
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        if let filterName {
            var filters = layer.filters ?? []
            guard let index = filters.firstIndex(where: { String(describing: $0) == filterName }),
                  let source = filters[index] as? NSCopying,
                  let filter = source.copy(with: nil) as? NSObject else { throw OpticalLayerError.missingFilter(name) }
            filter.setValue(0.0, forKey: parameter)
            if name == "lensShadow" { filter.setValue(0.0, forKey: "inputSDRShadowOpacity") }
            filters[index] = filter
            layer.filters = filters
        } else {
            layer.isHidden = true
        }
        if name == "paneHighlightAndGlobal" {
            try opticalLayer([0, 1, 0], named: "globalGlow", in: window).model().isHidden = true
        }
    }

    private func opticalLayer(_ path: [Int], named name: String, in window: UIWindow, fromWindow: Bool = false) throws -> CALayer {
        guard var layer = fromWindow ? window.layer : findTabBar(in: window)?.layer else { throw OpticalLayerError.missingLayer(name) }
        if fromWindow {
            while let parent = layer.superlayer { layer = parent }
        }
        if name == "paneKernel" {
            let matches = paneBackdrops(in: layer, minimumWidth: window.bounds.width * 0.8)
            guard matches.count == 1 else { throw OpticalLayerError.missingLayer("unique pane backdrop") }
            return matches[0].presentation() ?? matches[0]
        }
        for index in path {
            guard let children = layer.sublayers, children.indices.contains(index) else { throw OpticalLayerError.missingLayer(name) }
            layer = children[index]
        }
        return layer.presentation() ?? layer
    }

    private func paneBackdrops(in layer: CALayer, minimumWidth: CGFloat) -> [CALayer] {
        let matches = layer.bounds.width >= minimumWidth
            && (layer.filters ?? []).contains { String(describing: $0) == "glassBackground" }
        return (matches ? [layer] : []) + (layer.sublayers ?? []).flatMap {
            paneBackdrops(in: $0, minimumWidth: minimumWidth)
        }
    }

    private func decodeOpticalObject(_ data: Data) throws -> Any? {
        let decoder = try NSKeyedUnarchiver(forReadingFrom: data)
        decoder.requiresSecureCoding = false
        defer { decoder.finishDecoding() }
        return decoder.decodeObject(forKey: NSKeyedArchiveRootObjectKey)
    }

    private func signedOpticalMap(in window: UIWindow) throws -> Any {
        let layer = try opticalLayer([0, 0, 1, 0, 1, 0], named: "color matrix", in: window)
        guard let filter = layer.filters?.first(where: { String(describing: $0) == "colorMatrix" }) else { throw OpticalLayerError.missingFilter("color matrix") }
        let data = try NSKeyedArchiver.archivedData(withRootObject: filter, requiringSecureCoding: false)
        guard var archive = try PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
              var objects = archive["$objects"] as? [Any] else { throw OpticalLayerError.missingFilter("matrix archive") }
        for index in objects.indices {
            guard var matrix = objects[index] as? [String: Any], matrix["m11"] != nil else { continue }
            for row in 1...4 {
                for column in 1...5 {
                    matrix["m\(row)\(column)"] = row == column ? (row == 4 ? 1.0 : 0.25) : column == 5 && row < 4 ? 0.5 : 0.0
                }
            }
            objects[index] = matrix
        }
        archive["$objects"] = objects
        let mapped = try PropertyListSerialization.data(fromPropertyList: archive, format: .binary, options: 0)
        guard let copy = try decodeOpticalObject(mapped) else { throw OpticalLayerError.missingFilter("signed matrix") }
        return copy
    }

    private func exposeOpticalSDF(_ pattern: OpticalPattern, in window: UIWindow) throws {
        if ["chromaticKernel", "backgroundKernel", "paneKernel", "contentKernel", "restingBlurKernel"].contains(pattern.sourceLayer) {
            try exposeFilterKernel(pattern, in: window)
            return
        }
        let path: [Int]
        switch pattern.sourceLayer {
        case "innerShadowMask": path = [0, 0, 1, 0, 2, 0, 1]
        case "localGlowMask": path = [0, 1, 1]
        case "initialWarp": path = [0, 0, 1, 0, 0, 0]
        case "liftedWarp": path = [0, 0, 1, 0, 2, 0, 0, 0]
        case "contentWarp": path = [0, 0, 1, 0, 2, 1, 0, 0, 1, 0]
        case "highlight", "highlightTall": path = [0, 0, 1, 0, 2, 1, 0, 0, 3]
        case "backgroundField": path = [0, 0, 1, 0, 2, 1, 0, 0, 0, 0]
        case "foregroundField": path = [0, 0, 1, 0, 2, 1, 0, 0, 2, 0]
        case "paneHighlight", "paneHighlightTall": path = [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1]
        default: throw OpticalLayerError.unknownProbe(pattern.sourceLayer)
        }
        let source = try opticalLayer(path, named: pattern.sourceLayer, in: window, fromWindow: pattern.sourceLayer.hasPrefix("paneHighlight"))
        let copy = source.model()
        copy.removeFromSuperlayer()
        if pattern.sourceLayer.hasSuffix("Tall") {
            guard let effect = copy.value(forKey: "effect") else { throw OpticalLayerError.missingFilter("highlight effect") }
            let data = try NSKeyedArchiver.archivedData(withRootObject: effect, requiringSecureCoding: false)
            guard let expanded = try decodeOpticalObject(data) as? NSObject else { throw OpticalLayerError.missingFilter("highlight effect copy") }
            expanded.setValue(20.0, forKey: "keyHeight")
            expanded.setValue(20.0, forKey: "fillHeight")
            copy.setValue(expanded, forKey: "effect")
        }
        if pattern.sourceLayer == "innerShadowMask" {
            copy.filters = []
            if pattern.phase == 1 { copy.shadowOpacity = 1 }
        } else if pattern.sourceLayer == "localGlowMask" {
            copy.filters = []
            if pattern.phase == 1 { copy.opacity = 1 }
        } else if !pattern.sourceLayer.lowercased().contains("highlight") {
            copy.contentsFormat = .RGBA16Float
            copy.filters = [try signedOpticalMap(in: window)]
        } else {
            copy.filters = []
        }
        let container = CALayer()
        container.name = "Optical SDF profile"
        container.frame = window.bounds
        container.backgroundColor = ((pattern.phase == 0 && pattern.sourceLayer != "innerShadowMask") || pattern.sourceLayer == "localGlowMask" ? UIColor.black : UIColor.white).cgColor
        container.zPosition = 1000
        copy.position = CGPoint(x: container.bounds.midX, y: container.bounds.midY)
        container.addSublayer(copy)
        window.layer.addSublayer(container)
    }

    private func exposeFilterKernel(_ pattern: OpticalPattern, in window: UIWindow) throws {
        let content = pattern.sourceLayer == "contentKernel"
        let index = content ? 1 : pattern.sourceLayer == "backgroundKernel" ? 0 : 2
        let pane = pattern.sourceLayer == "paneKernel"
        let restingBlur = pattern.sourceLayer == "restingBlurKernel"
        let path = pane ? [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0]
            : restingBlur ? [0, 0, 1, 0, 1] : [0, 0, 1, 0, 2, 1, 0, 0, index]
        let source = try opticalLayer(path, named: pattern.sourceLayer, in: window, fromWindow: pane)
        var root = source
        while let parent = root.superlayer { root = parent }
        let frame = source.convert(source.bounds, to: root)
        let foreground = restingBlur ? CALayer() : source.model()
        if pane { try isolatePaneTransmission(foreground) }
        if restingBlur {
            let filters = (source.filters ?? []).filter { String(describing: $0) == "gaussianBlur" }
            guard filters.count == 1 else { throw OpticalLayerError.missingFilter("resting Gaussian blur") }
            foreground.bounds = source.bounds
            foreground.filters = filters
        }
        if let activity = pattern.contactActivity {
            if pattern.sourceLayer == "backgroundKernel" { try setOpticalBackgroundOpacity(foreground, opacity: activity) }
            else { try setOpticalContact(foreground, activity: activity) }
        }
        let format = UIGraphicsImageRendererFormat()
        format.scale = window.screen.scale
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: window.bounds.size, format: format)
        let background = renderer.image { context in
            pattern.drawStrips(size: window.bounds.size, origin: .zero, scale: format.scale) { rect, color in
                context.cgContext.setFillColor(UIColor(color).cgColor)
                context.cgContext.fill(rect)
            }
        }
        let container = CALayer()
        container.name = "Optical filter kernel"
        container.frame = window.bounds
        container.contents = background.cgImage
        container.contentsScale = format.scale
        container.zPosition = 1000
        foreground.removeFromSuperlayer()
        let scale = CGSize(width: frame.width / foreground.bounds.width, height: frame.height / foreground.bounds.height)
        foreground.setAffineTransform(CGAffineTransform(scaleX: scale.width, y: scale.height))
        foreground.position = CGPoint(x: frame.midX, y: frame.midY)
        if content || restingBlur {
            if content {
                guard let children = foreground.sublayers, children.count == 2 else { throw OpticalLayerError.missingLayer("content portal") }
                children[1].isHidden = true
            }
            let image = CALayer()
            image.name = "Optical content source"
            image.frame = CGRect(x: -frame.minX / scale.width, y: -frame.minY / scale.height,
                                 width: window.bounds.width / scale.width, height: window.bounds.height / scale.height)
            image.contents = background.cgImage
            image.contentsScale = format.scale
            foreground.addSublayer(image)
        }
        container.addSublayer(foreground)
        window.layer.addSublayer(container)
    }

    private func setOpticalBackgroundOpacity(_ layer: CALayer, opacity: Double) throws {
        var filters = layer.filters ?? []
        guard let index = filters.firstIndex(where: { String(describing: $0) == "glassBackground" }),
              let original = filters[index] as? NSCopying,
              let filter = original.copy(with: nil) as? NSObject else { throw OpticalLayerError.missingFilter("background opacity") }
        filter.setValue(opacity, forKey: "inputFaceOpacity")
        filters[index] = filter
        layer.filters = filters
    }

    private func setOpticalContact(_ layer: CALayer, activity: Double) throws {
        var filters = layer.filters ?? []
        guard let index = filters.firstIndex(where: { String(describing: $0) == "glassForeground" }),
              let original = filters[index] as? NSCopying,
              let filter = original.copy(with: nil) as? NSObject else { throw OpticalLayerError.missingFilter("contact foreground") }
        let values = ["inputEdgeOpacityStart": activity, "inputEdgeOpacityEnd": 1 - activity,
                      "inputEdgeStart": -14 * activity, "inputEdgeEnd": 0,
                      "inputRefractionHeight": 16 * (1 - activity), "inputRefractionOffset": -3.3 * (1 - activity),
                      "inputAberrationOffset": 38.88888931274414 * activity,
                      "inputAberrationAmount": -5 + 8.684210538864136 * activity,
                      "inputAberrationAngle": Double.pi / 2 - Double.pi * 7 / 12 * activity]
        for (key, value) in values { filter.setValue(value, forKey: key) }
        filters[index] = filter
        layer.filters = filters
    }

    private func isolatePaneTransmission(_ layer: CALayer) throws {
        var filters = layer.filters ?? []
        guard let index = filters.firstIndex(where: { String(describing: $0) == "glassBackground" }),
              let original = filters[index] as? NSCopying,
              let filter = original.copy(with: nil) as? NSObject else { throw OpticalLayerError.missingFilter("pane transmission") }
        for key in ["inputBlurRadius", "inputBlurFillNormalOpacity", "inputBlurFillLightenOpacity", "inputBlurFillDarkenOpacity", "inputFaceColorMatrixBlack", "inputShadowOpacity", "inputSDRShadowOpacity"] {
            filter.setValue(0.0, forKey: key)
        }
        filter.setValue(1.0, forKey: "inputFaceColorMatrixWhite")
        filter.setValue(1.0, forKey: "inputFaceColorMatrixSaturation")
        filter.setValue(UIColor.clear.cgColor, forKey: "inputFaceColorMatrixFillColor")
        filter.setValue(false, forKey: "inputSDRHoldingToneEnabled")
        filters[index] = filter
        layer.filters = filters
    }

    private func sampleTabBar(in window: UIWindow) -> [LayerSample] {
        guard let bar = findTabBar(in: window) else { return [] }
        let visibleBar = bar.layer.presentation() ?? bar.layer
        var root = visibleBar
        while let parent = root.superlayer { root = parent }
        var layers: [LayerSample] = []
        sample(visibleBar, root: root, path: "UITabBar", into: &layers)
        if optics.enabled || recordingFilters { sample(root, root: root, path: "Window", into: &layers) }
        return layers
    }

    private func recordContactFilters(in window: UIWindow, elapsed: TimeInterval) throws {
        recordingFilters = true
        defer {
            recordingFilters = false
            pendingContactFilters.removeAll(keepingCapacity: true)
            pendingContactEffects.removeAll(keepingCapacity: true)
        }
        var layers = sampleTabBar(in: window)
        let captured = CACurrentMediaTime() - began
        for (index, filters) in pendingContactFilters {
            layers[index].filterValues = filterValues(filters)
        }
        for (index, effect) in pendingContactEffects {
            layers[index].effectArchive = opticalArchive(effect)
        }
        let frame = MotionSample(timestamp: elapsed, targetTimestamp: captured, phase: phase, layers: layers)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.nonConformingFloatEncodingStrategy = .convertToString(positiveInfinity: "+Infinity", negativeInfinity: "-Infinity", nan: "NaN")
        let profile = ProcessInfo.processInfo.environment["REFERENCE_MATERIAL_PROFILE"].flatMap(Int.init)
        let tag = profile.map { "tint\($0)-" } ?? ""
        let name = "native-contact-filters-\(tag)\(Int((contactFilterTime ?? elapsed) * 1000)).json"
        try encoder.encode(frame).write(to: URL.documentsDirectory.appending(path: name), options: .atomic)
    }

    private func findTabBar(in view: UIView) -> UITabBar? {
        if let bar = view as? UITabBar { return bar }
        for child in view.subviews {
            if let bar = findTabBar(in: child) { return bar }
        }
        return nil
    }

    private func exportContent(_ bar: UITabBar) throws {
        var entries: [[String: Any]] = []
        func visit(_ view: UIView) throws {
            let rect = view.convert(view.bounds, to: window)
            var entry: [String: Any] = ["kind": String(describing: type(of: view)),
                                       "rect": [rect.minX, rect.minY, rect.width, rect.height]]
            entry["alpha"] = view.alpha
            entry["tintColor"] = String(describing: view.tintColor.resolvedColor(with: view.traitCollection))
            var ancestors: [[String: Any]] = []
            var parent: CALayer? = view.layer
            while let layer = parent {
                ancestors.append(["kind": String(describing: type(of: layer)),
                                  "opacity": layer.opacity,
                                  "compositingFilter": String(describing: layer.compositingFilter),
                                  "filters": String(describing: layer.filters)])
                parent = layer.superlayer
            }
            entry["compositingLayers"] = ancestors
            if let imageView = view as? UIImageView, let original = imageView.image,
               let image = imageView.preferredSymbolConfiguration.flatMap({ original.applyingSymbolConfiguration($0) }) ?? Optional(original),
               let png = image.pngData() {
                let name = "native-symbol-\(entries.count).png"
                try png.write(to: URL.documentsDirectory.appending(path: name), options: .atomic)
                entry["image"] = name
                entry["imageSize"] = [image.size.width, image.size.height]
                entry["scale"] = image.scale
                entry["configuration"] = String(describing: imageView.preferredSymbolConfiguration)
                entry["contentMode"] = imageView.contentMode.rawValue
                entries.append(entry)
            } else if let label = view as? UILabel {
                entry["text"] = label.text ?? ""
                entry["textColor"] = String(describing: label.textColor.resolvedColor(with: label.traitCollection))
                entry["font"] = label.font.fontName
                entry["fontSize"] = label.font.pointSize
                entry["fontAttributes"] = String(describing: label.font.fontDescriptor.fontAttributes)
                entry["fontVariations"] = String(describing: CTFontCopyVariation(label.font as CTFont))
                entry["glyphMetrics"] = glyphMetrics(label)
                entry["textRect"] = String(describing: label.textRect(forBounds: label.bounds, limitedToNumberOfLines: 1))
                entry["fontURL"] = String(describing: CTFontCopyAttribute(label.font as CTFont, kCTFontURLAttribute))
                if let attributed = label.attributedText, attributed.length > 0 {
                    entry["textAttributes"] = String(describing: attributed.attributes(at: 0, effectiveRange: nil))
                }
                entries.append(entry)
            }
            if ProcessInfo.processInfo.environment["REFERENCE_HIDE_CONTENT"] == "1",
               view is UIImageView || view is UILabel {
                view.alpha = 0
            }
            for child in view.subviews { try visit(child) }
        }
        try visit(bar)
        try JSONSerialization.data(withJSONObject: entries, options: [.prettyPrinted, .sortedKeys])
            .write(to: URL.documentsDirectory.appending(path: "native-content.json"), options: .atomic)
    }

    private func glyphMetrics(_ label: UILabel) -> [String: Any] {
        let attributed = NSMutableAttributedString(attributedString: label.attributedText ?? NSAttributedString(string: label.text ?? ""))
        attributed.addAttribute(.font, value: label.font as Any, range: NSRange(location: 0, length: attributed.length))
        let line = CTLineCreateWithAttributedString(attributed)
        var runs: [[String: Any]] = []
        for run in CTLineGetGlyphRuns(line) as! [CTRun] {
            let count = CTRunGetGlyphCount(run)
            var glyphs = [CGGlyph](repeating: 0, count: count)
            var positions = [CGPoint](repeating: .zero, count: count)
            var advances = [CGSize](repeating: .zero, count: count)
            CTRunGetGlyphs(run, CFRange(location: 0, length: 0), &glyphs)
            CTRunGetPositions(run, CFRange(location: 0, length: 0), &positions)
            CTRunGetAdvances(run, CFRange(location: 0, length: 0), &advances)
            runs.append(["glyphs": glyphs, "positions": positions.map { [$0.x, $0.y] },
                         "advances": advances.map { [$0.width, $0.height] }])
        }
        return ["width": CTLineGetTypographicBounds(line, nil, nil, nil), "runs": runs]
    }

    private func opticalArchive(_ object: Any) -> String? {
        guard object is NSCoding,
              let data = try? NSKeyedArchiver.archivedData(withRootObject: object, requiringSecureCoding: false) else { return nil }
        return data.base64EncodedString()
    }

    private func filterValues(_ layer: CALayer) -> [[String: String]] {
        let filters = (layer.filters ?? []) + (layer.backgroundFilters ?? []) + [layer.compositingFilter].compactMap { $0 }
        return filterValues(filters)
    }

    private func filterValues(_ filters: [Any]) -> [[String: String]] {
        filters.map { filter in
            var values = ["description": String(describing: filter), "kind": String(describing: type(of: filter))]
            values["archive"] = opticalArchive(filter)
            if let object = filter as? NSObject,
               object.responds(to: NSSelectorFromString("inputKeys")),
               let keys = object.value(forKey: "inputKeys") as? [String] {
                for key in keys { values[key] = String(describing: object.value(forKey: key)) }
            }
            return values
        }
    }

    private func animationValues(_ layer: CALayer) -> [[String: String]] {
        let model = layer.model()
        return (model.animationKeys() ?? []).compactMap { key in
            guard let animation = model.animation(forKey: key) else { return nil }
            var values = ["key": key, "kind": String(describing: type(of: animation)),
                          "beginTime": String(animation.beginTime), "duration": String(animation.duration),
                          "speed": String(animation.speed), "timeOffset": String(animation.timeOffset),
                          "description": animation.debugDescription]
            values["archive"] = opticalArchive(animation)
            if let spring = animation as? CASpringAnimation {
                values["mass"] = String(Double(spring.mass))
                values["stiffness"] = String(Double(spring.stiffness))
                values["damping"] = String(Double(spring.damping))
                values["initialVelocity"] = String(Double(spring.initialVelocity))
            }
            if let basic = animation as? CABasicAnimation {
                values["keyPath"] = basic.keyPath
                values["from"] = String(describing: basic.fromValue)
                values["to"] = String(describing: basic.toValue)
                values["by"] = String(describing: basic.byValue)
            }
            return values
        }
    }

    private func sample(_ layer: CALayer, root: CALayer, path: String, into samples: inout [LayerSample]) {
        let visible = layer
        let rect = visible.convert(visible.bounds, to: root)
        if recordingFilters {
            let filters = (visible.filters ?? []) + (visible.backgroundFilters ?? []) + [visible.compositingFilter].compactMap { $0 }
            pendingContactFilters.append((samples.count, filters.map { ($0 as? NSCopying)?.copy(with: nil) ?? $0 }))
            if visible.responds(to: NSSelectorFromString("effect")),
               let effect = visible.value(forKey: "effect") as? NSCopying {
                pendingContactEffects.append((samples.count, effect.copy(with: nil)))
            }
        }
        samples.append(LayerSample(path: path, kind: String(describing: type(of: layer)),
                                   rect: [rect.minX, rect.minY, rect.width, rect.height].allSatisfy(\.isFinite) ? [rect.minX, rect.minY, rect.width, rect.height] : nil,
                                   opacity: visible.opacity, cornerRadius: visible.cornerRadius,
                                   filterValues: optics.enabled ? filterValues(visible) : nil,
                                   descriptor: optics.enabled ? visible.debugDescription : nil,
                                   archive: optics.enabled && String(describing: type(of: visible)).hasPrefix("CASDF") ? opticalArchive(visible) : nil,
                                   bounds: [visible.bounds.minX, visible.bounds.minY, visible.bounds.width, visible.bounds.height],
                                   scale: [visible.transform.m11, visible.transform.m22],
                                   animations: recordingFilters ? animationValues(visible) : nil))
        for (index, child) in (layer.sublayers ?? []).enumerated() {
            sample(child, root: root, path: path + "/\(index)", into: &samples)
        }
    }
}

@MainActor
private final class PassiveTouchObserver: UIGestureRecognizer {
    private weak var trace: NativeTrace?

    init(trace: NativeTrace) {
        self.trace = trace
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
    }

    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { false }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first { trace?.receive(touch, phase: "Touch down") }
    }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first { trace?.receive(touch, phase: "Sliding") }
    }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first { trace?.receive(touch, phase: "Touch up") }
        state = .failed
    }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first { trace?.receive(touch, phase: "Cancelled") }
        state = .failed
    }
}

struct TouchProbe: UIViewRepresentable {
    let trace: NativeTrace

    func makeUIView(context: Context) -> ProbeView { ProbeView(trace: trace) }
    func updateUIView(_ uiView: ProbeView, context: Context) {
        if let window = uiView.window { trace.install(on: window) }
    }

    final class ProbeView: UIView {
        let trace: NativeTrace
        private var clock: CADisplayLink?
        init(trace: NativeTrace) {
            self.trace = trace
            super.init(frame: .zero)
            isUserInteractionEnabled = false
            backgroundColor = .white
            isOpaque = true
        }
        required init?(coder: NSCoder) { nil }
        override func didMoveToWindow() {
            super.didMoveToWindow()
            clock?.invalidate()
            clock = nil
            if let window { trace.install(on: window) }
            updateClock()
        }
        override func layoutSubviews() {
            super.layoutSubviews()
            updateClock()
        }
        private func updateClock() {
            guard window != nil, bounds.width > 0 else {
                clock?.invalidate()
                clock = nil
                return
            }
            guard clock == nil else { return }
            let link = CADisplayLink(target: self, selector: #selector(refresh))
            if let window { requestDisplayRefresh(link, screen: window.screen) }
            link.add(to: .main, forMode: .common)
            clock = link
        }
        @objc private func refresh() { setNeedsDisplay() }
        override func draw(_ rect: CGRect) {
            guard let context = UIGraphicsGetCurrentContext() else { return }
            let millis = UInt32(UInt64(Date().timeIntervalSince1970 * 1000) & 0xFFFFFF)
            let code = UInt32(0xB4) << 24 | millis
            for bit in 0..<32 {
                context.setFillColor(((code >> (31 - bit)) & 1 == 1 ? UIColor.white : .black).cgColor)
                context.fill(CGRect(x: CGFloat(bit) * 4, y: 0, width: 4, height: 8))
            }
        }
    }
}

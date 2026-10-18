// Counts the frames one app's window presents on macOS, from outside the app:
// macOS's own frame counter, the role SurfaceFlinger plays on Android.
//
// ScreenCaptureKit streams the window and marks a frame `complete` each time
// the window server composites the display again, which an app's present
// does and so does anything else that moves on that display (see `Frames`).
// A complete frame is the app's only when its pixels differ from the last
// one's, so that is what counts. The stream is scaled down to 1/8 to keep the
// capture's own cost small and alike for every app; at that scale the frames
// counted for GPUI, Slint and Cranpose are the drawables each took from its
// Metal layer, to within a frame a second.
//
// Usage: open -W -n FrameCount.app --args --pid PID --seconds S --out FILE.json
// or, from an app's launch to a deadline on the host clock
// (`CACurrentMediaTime`, the clock the frames are stamped with), waiting for
// its window to appear:
//        open -W -n FrameCount.app --args --pid PID --until T --out FILE.json
// With `--launched L --slow-fps F --slow-seconds S` as well, the capture ends
// early, its end written as `stopped`, once the app presented fewer than F
// frames in each of the last S whole seconds since its launch L.
// or, for a picture of the window at its full resolution:
//        open -W -n FrameCount.app --args --pid PID --screenshot FILE.png
// or, for the state of every display the window server knows and the rate at
// which its display link ticks on the main display, with no window involved:
//        open -W -n FrameCount.app --args --display FILE.json [--link-seconds S]
// or, for how often the window server composites the display with nothing but
// a window that never changes on it, which is how often something other than an
// app moves on the display (see `Frames`):
//        open -W -n FrameCount.app --args --ambient FILE.json [--seconds S]
// Launched through `open`, macOS attributes the capture to FrameCount.app
// itself, so one Screen Recording grant covers every caller.

import AppKit
import CoreGraphics
import CoreMedia
import CoreVideo
import Foundation
import ImageIO
import QuartzCore
import ScreenCaptureKit
import UniformTypeIdentifiers

struct Options {
    var pid: pid_t = 0
    var seconds = 5.0
    var until = 0.0
    var launched = 0.0
    var slowFps = 0.0
    var slowSeconds = 0
    var out = ""
    var screenshot = ""
    var display = ""
    var linkSeconds = 2.0
    var ambient = ""

    init(_ arguments: [String]) throws {
        var rest = arguments.dropFirst()[...]
        while let flag = rest.popFirst() {
            guard let value = rest.popFirst() else { throw Failure.usage }
            switch flag {
            case "--pid": pid = pid_t(value) ?? 0
            case "--seconds": seconds = Double(value) ?? 0
            case "--until": until = Double(value) ?? 0
            case "--launched": launched = Double(value) ?? 0
            case "--slow-fps": slowFps = Double(value) ?? 0
            case "--slow-seconds": slowSeconds = Int(value) ?? 0
            case "--out": out = value
            case "--screenshot": screenshot = value
            case "--display": display = value
            case "--link-seconds": linkSeconds = Double(value) ?? 0
            case "--ambient": ambient = value
            default: throw Failure.usage
            }
        }
        if !display.isEmpty {
            if linkSeconds <= 0 { throw Failure.usage }
            return
        }
        if !ambient.isEmpty {
            if seconds <= 0 { throw Failure.usage }
            return
        }
        if pid <= 0 || (seconds <= 0 && until <= 0) || (out.isEmpty && screenshot.isEmpty) { throw Failure.usage }
    }

    /// Whether, at `now`, each of the last `slowSeconds` whole seconds since
    /// the launch held fewer than `slowFps` of the frames `frames` counts
    /// between two times.
    func slow(at now: Double, frames: (Double, Double) -> Int) -> Bool {
        let done = Int(now - launched)
        guard slowFps > 0, slowSeconds > 0, done >= slowSeconds else { return false }
        return (done - slowSeconds..<done).allSatisfy { second in
            let start = launched + Double(second)
            return Double(frames(start, start + 1)) < slowFps
        }
    }
}

enum Failure: Error, CustomStringConvertible {
    case usage
    case noPermission
    case noWindow(pid_t)
    case unwritable(String)

    var description: String {
        switch self {
        case .usage:
            return "usage: FrameCount --pid PID (--seconds S --out FILE.json | --until T --out FILE.json"
                + " [--launched L --slow-fps F --slow-seconds S] | --screenshot FILE.png)"
                + " | FrameCount --display FILE.json [--link-seconds S]"
                + " | FrameCount --ambient FILE.json [--seconds S]"
        case .noPermission: return "FrameCount has no Screen Recording permission"
        case .noWindow(let pid): return "process \(pid) shows no window"
        case .unwritable(let path): return "cannot write \(path)"
        }
    }
}

/// The hash of a frame's pixels, which says whether two frames show the same
/// picture.
func pixelHash(_ buffer: CVPixelBuffer) -> UInt64 {
    CVPixelBufferLockBaseAddress(buffer, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
    guard let base = CVPixelBufferGetBaseAddress(buffer) else { return 0 }
    let rowBytes = CVPixelBufferGetBytesPerRow(buffer)
    let used = CVPixelBufferGetWidth(buffer) * 4
    var hash: UInt64 = 0xcbf2_9ce4_8422_2325
    for row in 0..<CVPixelBufferGetHeight(buffer) {
        let line = base.advanced(by: row * rowBytes)
        for word in 0..<(used / 8) {
            hash = (hash ^ line.loadUnaligned(fromByteOffset: word * 8, as: UInt64.self)) &* 0x100_0000_01b3
        }
        if used % 8 != 0 {
            let tail = UInt64(line.loadUnaligned(fromByteOffset: used - used % 8, as: UInt32.self))
            hash = (hash ^ tail) &* 0x100_0000_01b3
        }
    }
    return hash
}

/// Collects the presentation times of the frames the window server composited
/// and, when `hashing`, those among them that changed the window's picture.
///
/// ScreenCaptureKit marks a frame `complete` each time the window server
/// composites the display again, whether the window changed or not: a quiet
/// window in a corner of macm3's television was composited 34 times a second
/// while a Cranpose window animated in its middle, and 2 a second without it.
/// So the frames counted from the complete ones are those of whatever moves on
/// the display, a video or a cursor as much as the app (2026-10-18: GPUI, Flutter
/// and Slint counted 54 to 56 frames a second beside an animating window and
/// 15 to 25 alone, while a hook on GPUI's Metal layer counted 12 drawables a
/// second in both). A frame is the app's only when its pixels differ from the
/// last one's.
final class Frames: NSObject, SCStreamOutput {
    private let lock = NSLock()
    private let hashing: Bool
    private var times: [Double] = []
    private var changed: [Double] = []
    private var last: UInt64?

    init(hashing: Bool) {
        self.hashing = hashing
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]],
              let raw = attachments.first?[.status] as? Int,
              SCFrameStatus(rawValue: raw) == .complete
        else { return }
        let time = CMSampleBufferGetPresentationTimeStamp(sample).seconds
        let hash = hashing ? CMSampleBufferGetImageBuffer(sample).map(pixelHash) : nil
        lock.lock()
        defer { lock.unlock() }
        times.append(time)
        if let hash, hash != last {
            changed.append(time)
        }
        last = hash
    }

    /// The frames presented from `start` until `end`, counted from the newest.
    func count(from start: Double, until end: Double) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return (hashing ? changed : times).reversed().prefix { $0 >= start }.count { $0 < end }
    }

    /// Every frame composited.
    func composited() -> [Double] {
        lock.lock()
        defer { lock.unlock() }
        return times
    }

    /// The frames that count: those that changed the picture when hashing, else every one composited.
    func taken() -> [Double] {
        lock.lock()
        defer { lock.unlock() }
        return hashing ? changed : times
    }
}

func percentile(_ sorted: [Double], _ share: Double) -> Double {
    if sorted.isEmpty { return 0 }
    return sorted[min(sorted.count - 1, Int(share * Double(sorted.count)))]
}

/// Counts the ticks of one display's link, the clock that display-driven
/// frameworks pace their frames by. It ticks at the display's refresh rate
/// while the window server drives the display, and not otherwise.
final class Link {
    private let lock = NSLock()
    private var ticks: [Double] = []
    private var link: CVDisplayLink?
    /// The refresh rate the mode of the display asks for.
    let nominalHz: Double

    init?(display: CGDirectDisplayID) {
        var created: CVDisplayLink?
        guard CVDisplayLinkCreateWithCGDisplay(display, &created) == kCVReturnSuccess, let created else {
            return nil
        }
        let period = CVDisplayLinkGetNominalOutputVideoRefreshPeriod(created)
        nominalHz = period.timeValue > 0 ? Double(period.timeScale) / Double(period.timeValue) : 0
        link = created
        CVDisplayLinkSetOutputHandler(created) { [weak self] _, _, _, _, _ in
            self?.tick()
            return kCVReturnSuccess
        }
    }

    private func tick() {
        let now = CACurrentMediaTime()
        lock.lock()
        ticks.append(now)
        lock.unlock()
    }

    func start() {
        if let link { CVDisplayLinkStart(link) }
    }

    /// The tick times so far, on the host clock.
    func stop() -> [Double] {
        if let link { CVDisplayLinkStop(link) }
        lock.lock()
        defer { lock.unlock() }
        return ticks
    }
}

/// The ticks per second a link kept over `span` seconds, and the longest gap
/// between two ticks in milliseconds.
func linkRate(_ ticks: [Double], over span: Double) -> [String: Any] {
    let gaps = zip(ticks.dropFirst(), ticks).map { ($0 - $1) * 1000 }
    return ["ticks": ticks.count, "hz": span > 0 ? Double(ticks.count) / span : 0, "max_gap_ms": gaps.max() ?? 0]
}

func displays(_ fill: (UInt32, UnsafeMutablePointer<CGDirectDisplayID>?, UnsafeMutablePointer<UInt32>?) -> CGError)
    -> [CGDirectDisplayID]
{
    var count: UInt32 = 0
    _ = fill(0, nil, &count)
    var found = [CGDirectDisplayID](repeating: 0, count: Int(count))
    _ = fill(count, &found, &count)
    return Array(found.prefix(Int(count)))
}

/// What the window server says of one display: whether it drives it, the mode
/// it drives it in and the scale its windows are drawn at.
func describe(_ id: CGDirectDisplayID) -> [String: Any] {
    var entry: [String: Any] = [
        "id": Int(id),
        "main": id == CGMainDisplayID(),
        "active": CGDisplayIsActive(id) != 0,
        "asleep": CGDisplayIsAsleep(id) != 0,
        "builtin": CGDisplayIsBuiltin(id) != 0,
        "mirrors": CGDisplayMirrorsDisplay(id) != kCGNullDirectDisplay,
        "vendor": Int(CGDisplayVendorNumber(id)),
        "model": Int(CGDisplayModelNumber(id)),
    ]
    if let screen = NSScreen.screens.first(where: {
        ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value == id
    }) {
        entry["name"] = screen.localizedName
        entry["scale"] = screen.backingScaleFactor
        // The headroom above SDR white the display has now, and could have:
        // above 1, the window server composites in extended range.
        entry["edr"] = screen.maximumExtendedDynamicRangeColorComponentValue
        entry["edr_potential"] = screen.maximumPotentialExtendedDynamicRangeColorComponentValue
        entry["color_space"] = screen.colorSpace?.localizedName ?? ""
        entry["max_fps"] = screen.maximumFramesPerSecond
    }
    if let mode = CGDisplayCopyDisplayMode(id) {
        entry["points"] = [mode.width, mode.height]
        entry["pixels"] = [mode.pixelWidth, mode.pixelHeight]
        entry["refresh_hz"] = mode.refreshRate
    }
    return entry
}

/// Every display the window server knows, and the ticks of the main display's
/// link over `options.linkSeconds`.
func displayState(_ options: Options) async throws -> [String: Any] {
    let main = CGMainDisplayID()
    let link = Link(display: main)
    link?.start()
    let began = CACurrentMediaTime()
    try await Task.sleep(nanoseconds: UInt64(options.linkSeconds * 1_000_000_000))
    let span = CACurrentMediaTime() - began
    let ticks = link?.stop() ?? []
    let session = CGSessionCopyCurrentDictionary() as? [String: Any] ?? [:]
    return [
        "main": Int(main),
        "displays": displays { CGGetOnlineDisplayList($0, $1, $2) }.map(describe),
        "link": linkRate(ticks, over: span).merging(["nominal_hz": link?.nominalHz ?? 0]) { $1 },
        "on_console": session["kCGSSessionOnConsoleKey"] as? Bool ?? false,
        "screen_locked": session["CGSSessionScreenIsLocked"] as? Bool ?? false,
    ]
}

/// The largest window the process shows on screen.
func window(of pid: pid_t) async throws -> SCWindow {
    guard CGPreflightScreenCaptureAccess() else {
        // Lists FrameCount in System Settings for the user to allow.
        CGRequestScreenCaptureAccess()
        throw Failure.noPermission
    }
    let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
    let windows = content.windows.filter { $0.owningApplication?.processID == pid }
    guard let window = windows.max(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height })
    else { throw Failure.noWindow(pid) }
    return window
}

/// Writes the window's content at its full pixel resolution as a PNG.
func screenshot(_ options: Options) async throws {
    let window = try await window(of: options.pid)
    let filter = SCContentFilter(desktopIndependentWindow: window)
    let configuration = SCStreamConfiguration()
    configuration.width = Int(window.frame.width * CGFloat(filter.pointPixelScale))
    configuration.height = Int(window.frame.height * CGFloat(filter.pointPixelScale))
    configuration.showsCursor = false
    let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
    let url = URL(fileURLWithPath: options.screenshot) as CFURL
    guard let destination = CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil)
    else { throw Failure.unwritable(options.screenshot) }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { throw Failure.unwritable(options.screenshot) }
}

/// The process's window once it shows one, looked for every 20 ms until the
/// host clock reaches `until`.
func awaitedWindow(of pid: pid_t, until: Double) async throws -> SCWindow {
    while true {
        do {
            return try await window(of: pid)
        } catch Failure.noWindow(let pid) {
            if CACurrentMediaTime() >= until { throw Failure.noWindow(pid) }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
    }
}

/// A plain window the size of the gauntlet's, in the middle of `display`, shown
/// to the window server.
@MainActor
func showBlankWindow(on display: CGDirectDisplayID) -> NSWindow {
    let screen = NSScreen.screens.first(where: {
        ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value == display
    }) ?? NSScreen.screens[0]
    let size = NSSize(width: 1280, height: 820)
    let origin = NSPoint(x: screen.frame.midX - size.width / 2, y: screen.frame.midY - size.height / 2)
    let blank = NSWindow(contentRect: NSRect(origin: origin, size: size), styleMask: [.titled], backing: .buffered,
                         defer: false)
    blank.isReleasedWhenClosed = false
    blank.backgroundColor = NSColor(white: 0.93, alpha: 1)
    blank.orderFrontRegardless()
    // The window server shows the window once the run loop has committed it.
    RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.5))
    return blank
}

/// How often the window server composites a window that never changes, on the
/// main display where an app's window opens: the rate of whatever else moves
/// anywhere on that display, as a video, an animated wallpaper or another
/// app's window does. Without it the window is composited once.
func ambient(_ options: Options) async throws -> [String: Any] {
    let main = CGMainDisplayID()
    let blank = await showBlankWindow(on: main)
    defer { Task { @MainActor in blank.close() } }
    let window = try await awaitedWindow(of: ProcessInfo.processInfo.processIdentifier,
                                         until: CACurrentMediaTime() + 5)
    let configuration = SCStreamConfiguration()
    configuration.width = max(1, Int(window.frame.width / 8))
    configuration.height = max(1, Int(window.frame.height / 8))
    configuration.minimumFrameInterval = CMTime(value: 1, timescale: 480)
    configuration.queueDepth = 8
    configuration.showsCursor = false
    let frames = Frames(hashing: false)
    let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration,
                          delegate: nil)
    try stream.addStreamOutput(frames, type: .screen, sampleHandlerQueue: DispatchQueue(label: "frames"))
    try await stream.startCapture()
    // The window appears with a transition of its own: its frames are not what
    // moves under it.
    try await Task.sleep(nanoseconds: 1_000_000_000)
    let settled = frames.taken().count
    let began = CACurrentMediaTime()
    try await Task.sleep(nanoseconds: UInt64(options.seconds * 1_000_000_000))
    let span = CACurrentMediaTime() - began
    try await stream.stopCapture()
    let moved = frames.taken().count - settled
    var shown: CGDirectDisplayID = 0
    var shownCount: UInt32 = 0
    CGGetDisplaysWithRect(window.frame, 1, &shown, &shownCount)
    return ["fps": Double(moved) / span, "frames": moved, "seconds": span, "settled": settled,
            "display": describe(shownCount > 0 ? shown : main),
            "window": [window.frame.minX, window.frame.minY, window.frame.width, window.frame.height]]
}

func measure(_ options: Options) async throws -> [String: Any] {
    let window = options.until > 0
        ? try await awaitedWindow(of: options.pid, until: options.until)
        : try await window(of: options.pid)
    let configuration = SCStreamConfiguration()
    configuration.width = max(1, Int(window.frame.width / 8))
    configuration.height = max(1, Int(window.frame.height / 8))
    configuration.pixelFormat = kCVPixelFormatType_32BGRA
    configuration.minimumFrameInterval = CMTime(value: 1, timescale: 480)
    configuration.queueDepth = 8
    configuration.showsCursor = false
    let frames = Frames(hashing: true)
    let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration,
                          delegate: nil)
    try stream.addStreamOutput(frames, type: .screen, sampleHandlerQueue: DispatchQueue(label: "frames"))
    // The link of the display the window is on, ticking beside the capture:
    // it says whether the display ran at its rate while the window was counted.
    var shown: CGDirectDisplayID = 0
    var shownCount: UInt32 = 0
    CGGetDisplaysWithRect(window.frame, 1, &shown, &shownCount)
    let link = shownCount > 0 ? Link(display: shown) : nil
    try await stream.startCapture()
    link?.start()
    // Frames are stamped on the host clock: where the capture began on it
    // tells a caller which of its seconds the capture saw.
    let captureStart = CACurrentMediaTime()
    let seconds = options.until > 0 ? max(0, options.until - captureStart) : options.seconds
    let end = captureStart + seconds
    var stopped: Double?
    while CACurrentMediaTime() < end {
        let left = end - CACurrentMediaTime()
        try await Task.sleep(nanoseconds: UInt64(max(0, min(0.25, left)) * 1_000_000_000))
        let now = CACurrentMediaTime()
        if options.slow(at: now, frames: frames.count(from:until:)) {
            stopped = now
            break
        }
    }
    let linkTicks = link?.stop() ?? []
    let captured = CACurrentMediaTime() - captureStart
    try await stream.stopCapture()

    let times = frames.taken()
    let intervals = zip(times.dropFirst(), times).map { ($0 - $1) * 1000 }.sorted()
    let span = (times.last ?? 0) - (times.first ?? 0)
    var result: [String: Any] = [
        "pid": Int(options.pid),
        "window": [
            "x": window.frame.minX, "y": window.frame.minY,
            "width": window.frame.width, "height": window.frame.height, "on_screen": window.isOnScreen,
            "active": window.isActive, "layer": window.windowLayer,
            "frontmost": NSWorkspace.shared.frontmostApplication?.processIdentifier == options.pid,
        ],
        "seconds": seconds,
        "capture_start": captureStart,
        "frames": times.count,
        "fps": span > 0 ? Double(times.count - 1) / span : 0,
        "interval_p50_ms": percentile(intervals, 0.5),
        "interval_p99_ms": percentile(intervals, 0.99),
        "times": times,
        "composited": frames.composited().count,
        "display": describe(shown),
        "link": linkRate(linkTicks, over: captured).merging(["nominal_hz": link?.nominalHz ?? 0]) { $1 },
    ]
    if let stopped { result["stopped"] = stopped }
    return result
}

// Connects to the window server, which the capture needs: a launched app that
// never touches AppKit has no connection.
_ = NSApplication.shared
let options: Options
do {
    options = try Options(CommandLine.arguments)
} catch {
    FileHandle.standardError.write("\(error)\n".data(using: .utf8) ?? Data())
    exit(2)
}
do {
    if !options.display.isEmpty {
        let state = try await displayState(options)
        let json = try JSONSerialization.data(withJSONObject: state, options: [.prettyPrinted, .sortedKeys])
        try json.write(to: URL(fileURLWithPath: options.display))
    } else if !options.ambient.isEmpty {
        let result = try await ambient(options)
        let json = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try json.write(to: URL(fileURLWithPath: options.ambient))
    } else if !options.screenshot.isEmpty {
        try await screenshot(options)
    } else {
        let result = try await measure(options)
        let json = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try json.write(to: URL(fileURLWithPath: options.out))
    }
} catch {
    let report = [options.display, options.ambient, options.out].first { !$0.isEmpty } ?? ""
    if !report.isEmpty {
        let failure = try? JSONSerialization.data(withJSONObject: ["error": "\(error)"])
        try? failure?.write(to: URL(fileURLWithPath: report))
    }
    FileHandle.standardError.write("\(error)\n".data(using: .utf8) ?? Data())
    exit(1)
}

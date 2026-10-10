// Counts the frames one app's window presents on macOS, from outside the app:
// macOS's own frame counter, the role SurfaceFlinger plays on Android.
//
// ScreenCaptureKit streams the window and marks each frame `complete` when
// the window's content changed since the last one and `idle` when it did not;
// the complete frames are the frames the app presented. The stream is scaled
// down to keep the capture's own cost small and alike for every app.
//
// Usage: open -W -n FrameCount.app --args --pid PID --seconds S --out FILE.json
// or, from an app's launch to a deadline on the host clock
// (`CACurrentMediaTime`, the clock the frames are stamped with), waiting for
// its window to appear:
//        open -W -n FrameCount.app --args --pid PID --until T --out FILE.json
// or, for a picture of the window at its full resolution:
//        open -W -n FrameCount.app --args --pid PID --screenshot FILE.png
// Launched through `open`, macOS attributes the capture to FrameCount.app
// itself, so one Screen Recording grant covers every caller.

import AppKit
import CoreGraphics
import CoreMedia
import Foundation
import ImageIO
import QuartzCore
import ScreenCaptureKit
import UniformTypeIdentifiers

struct Options {
    var pid: pid_t = 0
    var seconds = 5.0
    var until = 0.0
    var out = ""
    var screenshot = ""

    init(_ arguments: [String]) throws {
        var rest = arguments.dropFirst()[...]
        while let flag = rest.popFirst() {
            guard let value = rest.popFirst() else { throw Failure.usage }
            switch flag {
            case "--pid": pid = pid_t(value) ?? 0
            case "--seconds": seconds = Double(value) ?? 0
            case "--until": until = Double(value) ?? 0
            case "--out": out = value
            case "--screenshot": screenshot = value
            default: throw Failure.usage
            }
        }
        if pid <= 0 || (seconds <= 0 && until <= 0) || (out.isEmpty && screenshot.isEmpty) { throw Failure.usage }
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
                + " | --screenshot FILE.png)"
        case .noPermission: return "FrameCount has no Screen Recording permission"
        case .noWindow(let pid): return "process \(pid) shows no window"
        case .unwritable(let path): return "cannot write \(path)"
        }
    }
}

/// Collects the presentation times of the frames whose content changed.
final class Frames: NSObject, SCStreamOutput {
    private let lock = NSLock()
    private var times: [Double] = []

    func stream(_ stream: SCStream, didOutputSampleBuffer sample: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen,
              let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: false)
                as? [[SCStreamFrameInfo: Any]],
              let raw = attachments.first?[.status] as? Int,
              SCFrameStatus(rawValue: raw) == .complete
        else { return }
        let time = CMSampleBufferGetPresentationTimeStamp(sample).seconds
        lock.lock()
        times.append(time)
        lock.unlock()
    }

    func taken() -> [Double] {
        lock.lock()
        defer { lock.unlock() }
        return times
    }
}

func percentile(_ sorted: [Double], _ share: Double) -> Double {
    if sorted.isEmpty { return 0 }
    return sorted[min(sorted.count - 1, Int(share * Double(sorted.count)))]
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

func measure(_ options: Options) async throws -> [String: Any] {
    let window = options.until > 0
        ? try await awaitedWindow(of: options.pid, until: options.until)
        : try await window(of: options.pid)
    let configuration = SCStreamConfiguration()
    configuration.width = max(1, Int(window.frame.width / 8))
    configuration.height = max(1, Int(window.frame.height / 8))
    configuration.minimumFrameInterval = CMTime(value: 1, timescale: 480)
    configuration.queueDepth = 8
    configuration.showsCursor = false
    let frames = Frames()
    let stream = SCStream(filter: SCContentFilter(desktopIndependentWindow: window), configuration: configuration,
                          delegate: nil)
    try stream.addStreamOutput(frames, type: .screen, sampleHandlerQueue: DispatchQueue(label: "frames"))
    try await stream.startCapture()
    // Frames are stamped on the host clock: where the capture began on it
    // tells a caller which of its seconds the capture saw.
    let captureStart = CACurrentMediaTime()
    let seconds = options.until > 0 ? max(0, options.until - captureStart) : options.seconds
    try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
    try await stream.stopCapture()

    let times = frames.taken()
    let intervals = zip(times.dropFirst(), times).map { ($0 - $1) * 1000 }.sorted()
    let span = (times.last ?? 0) - (times.first ?? 0)
    return [
        "pid": Int(options.pid),
        "window": ["width": window.frame.width, "height": window.frame.height],
        "seconds": seconds,
        "capture_start": captureStart,
        "frames": times.count,
        "fps": span > 0 ? Double(times.count - 1) / span : 0,
        "interval_p50_ms": percentile(intervals, 0.5),
        "interval_p99_ms": percentile(intervals, 0.99),
        "times": times,
    ]
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
    if !options.screenshot.isEmpty {
        try await screenshot(options)
    } else {
        let result = try await measure(options)
        let json = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
        try json.write(to: URL(fileURLWithPath: options.out))
    }
} catch {
    if !options.out.isEmpty {
        let failure = try? JSONSerialization.data(withJSONObject: ["error": "\(error)"])
        try? failure?.write(to: URL(fileURLWithPath: options.out))
    }
    FileHandle.standardError.write("\(error)\n".data(using: .utf8) ?? Data())
    exit(1)
}

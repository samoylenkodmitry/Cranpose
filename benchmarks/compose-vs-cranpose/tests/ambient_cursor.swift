// Run on the idle benchmark Mac with the pointer near the center of the main display.
// swift tests/ambient_cursor.swift /path/to/FrameCount.app
import CoreGraphics
import Foundation

struct Reading: Decodable {
    let fps: Double
    let window: [CGFloat]
}

enum TestFailure: Error { case probeFailed, pointerOutsideProbe, noisyProbe }

func checkQuietWindow(app: URL) throws {
    let output = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
    defer { try? FileManager.default.removeItem(at: output) }
    let pointer = CGEvent(source: nil)?.location
    let probe = Process()
    probe.executableURL = URL(fileURLWithPath: "/usr/bin/open")
    probe.arguments = ["-W", "-n", app.path, "--args", "--ambient", output.path, "--seconds", "3"]
    try probe.run()
    probe.waitUntilExit()
    guard probe.terminationStatus == 0 else { throw TestFailure.probeFailed }
    let reading = try JSONDecoder().decode(Reading.self, from: Data(contentsOf: output))
    guard reading.window.count == 4 else { throw TestFailure.probeFailed }
    let bounds = CGRect(x: reading.window[0], y: reading.window[1], width: reading.window[2], height: reading.window[3])
    guard let pointer, bounds.contains(pointer), CGEvent(source: nil)?.location == pointer else {
        throw TestFailure.pointerOutsideProbe
    }
    print("Ambient window with pointer inside: \(reading.fps) composites/s")
    guard reading.fps <= 5 else { throw TestFailure.noisyProbe }
}

do {
    guard CommandLine.arguments.count == 2 else {
        fputs("Usage: swift ambient_cursor.swift /path/to/FrameCount.app\n", stderr)
        exit(2)
    }
    try checkQuietWindow(app: URL(fileURLWithPath: CommandLine.arguments[1]))
} catch {
    fputs("\(error)\n", stderr)
    exit(1)
}

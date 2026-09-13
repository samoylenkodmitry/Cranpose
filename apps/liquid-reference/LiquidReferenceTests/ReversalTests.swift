import XCTest

private struct RecordedPointerEvent: Decodable {
    let eventType: Int
    let offset: Double
    let x: Double
    let y: Double

    enum CodingKeys: String, CodingKey {
        case eventType, offset
        case x = "coordinate.x"
        case y = "coordinate.y"
    }
}

private enum RecordedPointerPathError: Error { case invalidPath }

private func recordedPointerPath(_ json: String, route: Int, count: Int) throws -> ([NSValue], [NSNumber]) {
    let paths = try JSONDecoder().decode([[RecordedPointerEvent]].self, from: Data(json.utf8))
    guard paths.count == count, paths.indices.contains(route) else { throw RecordedPointerPathError.invalidPath }
    let events = paths[route]
    guard events.count >= 2, events.first?.eventType == 1, events.last?.eventType == 3,
          events.dropFirst().dropLast().allSatisfy({ $0.eventType == 2 }),
          events.allSatisfy({ $0.x.isFinite && $0.y.isFinite && $0.offset.isFinite }),
          events.first?.offset == 0,
          zip(events, events.dropFirst()).allSatisfy({ $0.offset < $1.offset })
    else { throw RecordedPointerPathError.invalidPath }
    return (events.map { NSValue(cgPoint: CGPoint(x: $0.x, y: $0.y)) }, events.map { NSNumber(value: $0.offset) })
}

@MainActor
class NativeReversalTests: TabBarTests {
    var tintAmount: Int? { nil }
    var usesSystemTint: Bool { true }
    var contentCases: [[String: Any]] { [[:]] }

    func runReversal(content: [String: Any], tintAmount: Int?, route: Int) {
        let titles = content["titles"] as? [String] ?? ["Discover", "Browse", "Saved", "Account"]
        let json = content.isEmpty ? nil : String(data: try! JSONSerialization.data(withJSONObject: content, options: [.sortedKeys]), encoding: .utf8)
        let app = launch(initial: 3, settlingSeconds: 2, materialProfile: tintAmount, content: json, firstTitle: titles[0])
        if let tintAmount, !usesSystemTint {
            XCTAssertTrue(app.staticTexts["Glass tint: \(tintAmount)%"].exists)
        }
        assertDestination(titles[3], app: app)
        capture("gesture-rest", app: app)
        if route == 0 {
            let viewport = XCTAttachment(string: "{\"width\":\(app.frame.width),\"height\":\(app.frame.height),\"settlingSeconds\":2}")
            viewport.name = "gesture-viewport"
            viewport.lifetime = .keepAlways
            add(viewport)
        }
        let left = app.buttons[titles[0]].frame
        let right = app.buttons[titles[3]].frame
        let a = CGPoint(x: right.midX, y: right.midY)
        let b = CGPoint(x: left.midX, y: left.midY)
        let distance = abs(a.x - b.x)
        let first = 0.8 + distance / 1000
        let second = first + distance / 250
        let third = second + distance / 1000
        var points = [a, a, b, a, b, b].map { NSValue(cgPoint: $0) }
        var offsets = [0, 0.8, first, second, third, third + 1.2].map { NSNumber(value: $0) }
        XCTContext.runActivity(named: "Reference pointer synthesis") { _ in
            do {
                if let json = ProcessInfo.processInfo.environment["REFERENCE_POINTER_PATHS"] {
                    (points, offsets) = try recordedPointerPath(json, route: route, count: contentCases.count)
                    XCTAssertTrue(right.contains(points.first!.cgPointValue))
                    XCTAssertTrue(left.contains(points.last!.cgPointValue))
                }
                let data = try ReferenceGesture.synthesize(points: points, offsets: offsets,
                                                            name: "Continuous left right left")
                let path = XCTAttachment(data: data, uniformTypeIdentifier: "public.data")
                path.name = "Reference pointer path"
                path.lifetime = .keepAlways
                add(path)
            } catch {
                XCTFail("Continuous gesture failed: \(error)")
            }
        }
        Thread.sleep(forTimeInterval: 2)
        assertDestination(titles[0], app: app)
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
        capture("reversal-released", app: app)
    }

    override func testInteractionKeyframes() {
        continueAfterFailure = false
        if let tintAmount {
            if usesSystemTint {
                addTeardownBlock { @MainActor [self] in
                    continueAfterFailure = true
                    setSystemGlassTint(25)
                }
                setSystemGlassTint(tintAmount)
            }
            let data = try! JSONSerialization.data(withJSONObject: ["glassTintPercent": tintAmount, "contentCases": contentCases], options: [.sortedKeys])
            let profile = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
            profile.name = "reference-profile"
            profile.lifetime = .keepAlways
            add(profile)
        }
        for (index, content) in contentCases.enumerated() {
            runReversal(content: content, tintAmount: tintAmount, route: index)
        }
    }

    func testInvalidContinuousPathsAreRejectedBeforeSynthesis() {
        let point = NSValue(cgPoint: CGPoint(x: 10, y: 10))
        let invalid: [[NSNumber]] = [[], [0, 0.8, 0.7, 2], [0, 0.8, .init(value: Double.nan), 2]]
        for offsets in invalid {
            XCTAssertThrowsError(try ReferenceGesture.synthesize(points: Array(repeating: point, count: offsets.count),
                                                                  offsets: offsets, name: "Invalid path"))
        }
    }

    func testRecordedPathsRetainCoordinatesAndRejectInvalidEvents() throws {
        let events: [[String: Any]] = [
            ["eventType": 1, "offset": 0, "coordinate.x": 100.25, "coordinate.y": 50],
            ["eventType": 2, "offset": 0.5, "coordinate.x": 25.75, "coordinate.y": 50],
            ["eventType": 3, "offset": 1, "coordinate.x": 25.75, "coordinate.y": 50]]
        func encoded(_ path: [[String: Any]]) throws -> String {
            String(data: try JSONSerialization.data(withJSONObject: [path]), encoding: .utf8)!
        }
        let json = try encoded(events)
        let (points, offsets) = try recordedPointerPath(json, route: 0, count: 1)
        XCTAssertEqual(points.map(\.cgPointValue), [CGPoint(x: 100.25, y: 50), CGPoint(x: 25.75, y: 50), CGPoint(x: 25.75, y: 50)])
        XCTAssertEqual(offsets, [0, 0.5, 1])
        XCTAssertThrowsError(try recordedPointerPath(json, route: 1, count: 1))
        XCTAssertThrowsError(try recordedPointerPath(json, route: 0, count: 2))
        for (index, key, value) in [(0, "eventType", 2), (1, "eventType", 3), (2, "eventType", 2), (1, "offset", 0)] {
            var invalid = events
            invalid[index][key] = value
            XCTAssertThrowsError(try recordedPointerPath(encoded(invalid), route: 0, count: 1))
        }
    }
}

@MainActor
final class NativeTint0Tests: NativeReversalTests {
    override var tintAmount: Int? { 0 }
}

@MainActor
final class NativeTint25Tests: NativeReversalTests {
    override var tintAmount: Int? { 25 }
}

@MainActor
final class NativeTint50Tests: NativeReversalTests {
    override var tintAmount: Int? { 50 }
}

@MainActor
final class NativeTint100Tests: NativeReversalTests {
    override var tintAmount: Int? { 100 }
}

@MainActor
class CranposeReversalTests: NativeReversalTests {
    override var usesSystemTint: Bool { false }
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor
final class CranposeTint25Tests: CranposeReversalTests {
    override var tintAmount: Int? { 25 }
}

@MainActor
final class CranposeTint0Tests: CranposeReversalTests {
    override var tintAmount: Int? { 0 }
}

@MainActor
final class CranposeTint50Tests: CranposeReversalTests {
    override var tintAmount: Int? { 50 }
}

@MainActor
final class CranposeTint100Tests: CranposeReversalTests {
    override var tintAmount: Int? { 100 }
}

@MainActor
final class CranposeAppearanceTests: TabBarTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }

    func testTintButtonsUpdateTheFrameworkTheme() {
        let app = launch(materialProfile: 25)
        for percent in [0, 20, 50, 75, 100, 25] {
            app.buttons["\(percent)%"].tap()
            XCTAssertTrue(app.staticTexts["Glass tint: \(percent)%"].exists)
        }
        capture("cranpose-tint-controls", app: app)
    }
}

@MainActor
class NativeMatrixTests: NativeReversalTests {
    override var contentCases: [[String: Any]] {
        [[:],
         ["titles": ["A", "Library", "Favorites", "Profile"], "icons": [3, 2, 0, 1],
          "accent": [0.85, 0.1, 0.45], "palette": [[0.1, 0.05, 0.3], [0.9, 0.7, 0.15], [0.15, 0.7, 0.65], [0.8, 0.15, 0.05]]],
         ["titles": ["Inbox", "WWW", "II", "Settings"], "icons": [1, 3, 2, 0],
          "accent": [0.2, 0.65, 0.1], "palette": [[0.04, 0.04, 0.04], [0.9, 0.9, 0.9], [0.35, 0.35, 0.35]]]]
    }
}

@MainActor
class CranposeMatrixTests: NativeMatrixTests {
    override var usesSystemTint: Bool { false }
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class NativeMatrix0Tests: NativeMatrixTests { override var tintAmount: Int? { 0 } }
@MainActor final class NativeMatrix20Tests: NativeMatrixTests { override var tintAmount: Int? { 20 } }
@MainActor final class NativeMatrix50Tests: NativeMatrixTests { override var tintAmount: Int? { 50 } }
@MainActor final class NativeMatrix75Tests: NativeMatrixTests { override var tintAmount: Int? { 75 } }
@MainActor final class NativeMatrix100Tests: NativeMatrixTests { override var tintAmount: Int? { 100 } }
@MainActor final class CranposeMatrix0Tests: CranposeMatrixTests { override var tintAmount: Int? { 0 } }
@MainActor final class CranposeMatrix20Tests: CranposeMatrixTests { override var tintAmount: Int? { 20 } }
@MainActor final class CranposeMatrix50Tests: CranposeMatrixTests { override var tintAmount: Int? { 50 } }
@MainActor final class CranposeMatrix75Tests: CranposeMatrixTests { override var tintAmount: Int? { 75 } }
@MainActor final class CranposeMatrix100Tests: CranposeMatrixTests { override var tintAmount: Int? { 100 } }

@MainActor
final class NativeReleasedFilterTests: TabBarTests {
    func testReleasedBackdrop() {
        let app = launch(initial: 3, settlingSeconds: 2, contactFilterTime: 0.9)
        app.buttons["Account"].press(forDuration: 0.2)
        Thread.sleep(forTimeInterval: 2)
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
        capture("released-blur", app: app)
    }
}

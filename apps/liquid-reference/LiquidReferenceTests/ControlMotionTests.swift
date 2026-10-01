import XCTest

@MainActor
class NativeSliderMotionTests: ReferenceUITests {
    var component: String { "slider" }
    var initialValue: String { "0.5" }
    var positions: [CGFloat] { [201, 280, 122, 280] }
    var offsets: [NSNumber] { [0, 0.6, 0.9, 1.5, 1.8, 2.6] }

    func testInteractionKeyframes() throws {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
        app.launchEnvironment = ["REFERENCE_COMPONENT": component,
                                 "REFERENCE_VALUE": initialValue,
                                 "REFERENCE_SCHEME": "light",
                                 "REFERENCE_BACKDROP": "solid",
                                 "REFERENCE_RECORDING": "1",
                                 "REFERENCE_CAPTURE_GESTURES": "1",
                                 "REFERENCE_SETTLING_SECONDS": "2"]
        app.launch()
        XCTAssertTrue(app.staticTexts["Reference control"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.frame.size, CGSize(width: 402, height: 874))
        capture("gesture-rest", app: app)
        let viewport = XCTAttachment(string: "{\"width\":402,\"height\":874,\"settlingSeconds\":2,\"crop\":[0,351,402,200],\"component\":\"\(component)\"}")
        viewport.name = "gesture-viewport"
        viewport.lifetime = .keepAlways
        add(viewport)
        let points = [positions[0], positions[0], positions[1], positions[2], positions[3], positions[3]]
            .map { NSValue(cgPoint: CGPoint(x: $0, y: 451)) }
        try XCTContext.runActivity(named: "Reference pointer synthesis") { _ in
            let data = try ReferenceGesture.synthesize(points: points, offsets: offsets,
                                                       name: "Control hold right left right release")
            let path = XCTAttachment(data: data, uniformTypeIdentifier: "public.data")
            path.name = "Reference pointer path"
            path.lifetime = .keepAlways
            add(path)
        }
        Thread.sleep(forTimeInterval: 2.2)
        capture("control-motion-released", app: app)
        if component == "toggle" {
            let enabled = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Enabled")).firstMatch
            XCTAssertTrue(["1", "on"].contains(enabled.value as? String ?? ""))
        } else if component == "segmented" {
            XCTAssertTrue(app.buttons["Saved"].isSelected)
        }
    }
}

@MainActor class NativeToggleMotionTests: NativeSliderMotionTests {
    override var component: String { "toggle" }
    override var initialValue: String { "0" }
    override var positions: [CGFloat] { [190, 214, 190, 214] }
}

@MainActor class NativeSegmentedMotionTests: NativeSliderMotionTests {
    override var component: String { "segmented" }
    override var initialValue: String { "0" }
    override var positions: [CGFloat] { [101, 301, 101, 301] }
}

@MainActor final class CranposeSliderMotionTests: NativeSliderMotionTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class NativeSliderHoldoutMotionTests: NativeSliderMotionTests {
    override var positions: [CGFloat] { [201, 122, 280, 201] }
}

@MainActor final class NativeSliderSlowReleaseMotionTests: NativeSliderMotionTests {
    override var offsets: [NSNumber] { [0, 0.6, 0.9, 1.5, 2.7, 3.5] }
}

@MainActor final class NativeSliderMediumReleaseMotionTests: NativeSliderMotionTests {
    override var offsets: [NSNumber] { [0, 0.6, 0.9, 1.5, 2.1, 2.9] }
}

@MainActor final class NativeSliderGentleReleaseMotionTests: NativeSliderMotionTests {
    override var offsets: [NSNumber] { [0, 0.6, 0.9, 1.5, 2.4, 3.2] }
}

@MainActor final class NativeSliderThresholdReleaseMotionTests: NativeSliderMotionTests {
    override var offsets: [NSNumber] { [0, 0.6, 0.9, 1.5, 2.5, 3.3] }
}

@MainActor final class NativeSliderTrackTouchMotionTests: NativeSliderMotionTests {
    override var positions: [CGFloat] { [122, 122, 122, 122] }
}

@MainActor final class NativeSliderTrackDragMotionTests: NativeSliderMotionTests {
    override var positions: [CGFloat] { [122, 201, 280, 201] }
}

@MainActor final class NativeSliderEdgeMotionTests: NativeSliderMotionTests {
    override var positions: [CGFloat] { [232, 280, 122, 280] }
}

@MainActor final class CranposeSliderHoldoutMotionTests: NativeSliderMotionTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
    override var positions: [CGFloat] { [201, 122, 280, 201] }
}

@MainActor final class CranposeToggleMotionTests: NativeToggleMotionTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class CranposeSegmentedMotionTests: NativeSegmentedMotionTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class NativeControlAnimationTests: ReferenceUITests {
    func testAnimationParameters() {
        for (component, x) in [("slider", 201.0), ("toggle", 190.0), ("segmented", 101.0)] {
            let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
            app.launchEnvironment = ["REFERENCE_COMPONENT": component,
                                     "REFERENCE_VALUE": component == "slider" ? "0.5" : "0",
                                     "REFERENCE_BACKDROP": "solid",
                                     "REFERENCE_RECORDING": "1",
                                     "REFERENCE_CAPTURE_ANIMATIONS": "1",
                                     "REFERENCE_CONTACT_FILTER_TIME": "0.1"]
            app.launch()
            XCTAssertTrue(app.staticTexts["Reference control"].waitForExistence(timeout: 10))
            app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: x, dy: 451)).press(forDuration: 0.5)
            Thread.sleep(forTimeInterval: 1)
            app.terminate()
        }
    }
}

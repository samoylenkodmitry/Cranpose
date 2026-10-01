import XCTest

@MainActor
class FloatingControlTests: ReferenceUITests {
    var backdrop: String { "checkerboard-mono" }
    var captureColorMatrices: Bool { false }

    func captureFloating(_ component: String, selected: Bool = false, dragOutside: Bool = false, probe: String? = nil, pressOffset: CGFloat = 0) throws {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
        app.launchEnvironment = ["REFERENCE_COMPONENT": component,
                                 "REFERENCE_VALUE": selected ? "1" : "0",
                                 "REFERENCE_SCHEME": "light",
                                 "REFERENCE_BACKDROP": backdrop,
                                 "REFERENCE_RECORDING": "1",
                                 "REFERENCE_CAPTURE_GESTURES": "1",
                                 "REFERENCE_CAPTURE_ANIMATIONS": "1",
                                 "REFERENCE_CAPTURE_CASE": "\(selected ? "on" : "off")-\(dragOutside ? "drag-out" : "press")",
                                 "REFERENCE_CONTACT_FILTER_TIME": "0.4",
                                  "REFERENCE_SETTLING_SECONDS": "2"]
        if captureColorMatrices {
            app.launchEnvironment["REFERENCE_CAPTURE_COLOR_MATRICES"] = "1"
        }
        if let probe {
            app.launchEnvironment["REFERENCE_CONTROL_OPTICAL_PROBE"] = probe
            app.launchEnvironment["REFERENCE_CAPTURE_CASE"] = "probe-\(probe)"
        }
        if pressOffset != 0 {
            app.launchEnvironment["REFERENCE_CAPTURE_CASE"] = "offset-\(pressOffset)"
        }
        app.launch()
        XCTAssertTrue(app.staticTexts["Reference control"].waitForExistence(timeout: 10))
        XCTAssertEqual(app.frame.size, CGSize(width: 402, height: 874))
        XCTAssertTrue(app.staticTexts["Activations: 0"].exists)
        capture("gesture-rest", app: app)
        let expectedActivations = !dragOutside || component == "button" || component == "prominent-button" ? 1 : 0
        let viewport: [String: Any] = ["width": 402, "height": 874, "settlingSeconds": 2,
                                      "crop": [0, 351, 402, 200], "component": component,
                                      "backdrop": backdrop, "checkerCellPoints": 8,
                                      "checkerOriginPoints": [0, 0], "initialSelected": selected,
                                      "expectedActivations": expectedActivations,
                                      "interaction": dragOutside ? "drag-out-release" : "press-hold-release"]
        let geometry = XCTAttachment(data: try JSONSerialization.data(withJSONObject: viewport, options: [.sortedKeys]),
                                     uniformTypeIdentifier: "public.json")
        geometry.name = "gesture-viewport"
        geometry.lifetime = .keepAlways
        add(geometry)
        let positions: [CGFloat] = dragOutside ? [201, 201, 301, 321, 321, 321] : [201, 201, 213, 189, 201, 201]
        try XCTContext.runActivity(named: "Reference pointer synthesis") { _ in
            let data = try ReferenceGesture.synthesize(
                 points: positions.map { NSValue(cgPoint: CGPoint(x: $0 + pressOffset, y: 451)) },
                offsets: [0, 0.6, 0.9, 1.2, 1.5, 2.2],
                name: dragOutside ? "Floating control drag outside and release" : "Floating control hold and activate")
            let path = XCTAttachment(data: data, uniformTypeIdentifier: "public.data")
            path.name = "Reference pointer path"
            path.lifetime = .keepAlways
            add(path)
        }
        Thread.sleep(forTimeInterval: 2.2)
        capture("floating-released", app: app)
        XCTAssertTrue(app.staticTexts["Activations: \(expectedActivations)"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
    }
}

@MainActor class NativeChipTransitionTests: FloatingControlTests {
    override var captureColorMatrices: Bool { true }
    func testSelect() throws { try captureFloating("filter-chip") }
    func testDeselect() throws { try captureFloating("filter-chip", selected: true) }
}

@MainActor final class NativeRainbowChipTransitionTests: NativeChipTransitionTests {
    override var backdrop: String { "checkerboard" }
}

@MainActor class NativeFloatingMonoTests: FloatingControlTests {
    func testActionChipPress() throws { try captureFloating("chip") }
    func testActionChipDragOut() throws { try captureFloating("chip", dragOutside: true) }
    func testFilterChipPress() throws { try captureFloating("filter-chip") }
    func testFilterChipDragOut() throws { try captureFloating("filter-chip", dragOutside: true) }
    func testSelectedFilterChipPress() throws { try captureFloating("filter-chip", selected: true) }
    func testSelectedFilterChipDragOut() throws { try captureFloating("filter-chip", selected: true, dragOutside: true) }
    func testGlassButtonPress() throws { try captureFloating("button") }
    func testGlassButtonDragOut() throws { try captureFloating("button", dragOutside: true) }
    func testProminentButtonPress() throws { try captureFloating("prominent-button") }
    func testProminentButtonDragOut() throws { try captureFloating("prominent-button", dragOutside: true) }
    func testIconButtonPress() throws { try captureFloating("icon-button") }
    func testIconButtonDragOut() throws { try captureFloating("icon-button", dragOutside: true) }
}

@MainActor final class NativeFloatingProbeTests: FloatingControlTests {
    func testChipWithoutOuter() throws { try captureFloating("chip", probe: "outer-surface") }
    func testChipWithoutInner() throws { try captureFloating("chip", probe: "surface") }
    func testButtonWithoutOuter() throws { try captureFloating("button", probe: "outer-surface") }
    func testButtonWithoutInner() throws { try captureFloating("button", probe: "surface") }
    func testButtonWithoutGlobalLight() throws { try captureFloating("button", probe: "floating-global") }
    func testButtonWithoutLocalLight() throws { try captureFloating("button", probe: "floating-local") }
    func testButtonWithoutRefractionMix() throws { try captureFloating("button", probe: "refraction-opacity") }
    func testButtonWithoutFillBlur() throws { try captureFloating("button", probe: "fill-blur") }
    func testButtonHalfFillBlur() throws { try captureFloating("button", probe: "fill-blur-half") }
}

@MainActor final class NativeFloatingTintProbeTests: FloatingControlTests {
    func testProminentWithoutLocalLight() throws { try captureFloating("prominent-button", probe: "floating-local") }
    func testProminentWithoutGlobalLight() throws { try captureFloating("prominent-button", probe: "floating-global") }
    func testProminentPlainLocalLight() throws { try captureFloating("prominent-button", probe: "floating-local-plain") }
    func testButtonPlainLocalLight() throws { try captureFloating("button", probe: "floating-local-plain") }
}

@MainActor final class NativeFloatingRimTests: FloatingControlTests {
    func testButtonWithoutRim() throws { try captureFloating("button", probe: "floating-rim") }
    func testProminentWithoutRim() throws { try captureFloating("prominent-button", probe: "floating-rim") }
    func testIconWithoutRim() throws { try captureFloating("icon-button", probe: "floating-rim") }
}

@MainActor final class NativeFloatingFlatColorTests: FloatingControlTests {
    var color = "rgb-1,0,0"
    override var backdrop: String { color }
    func testChipRed() throws { color = "rgb-1,0,0"; try captureFloating("chip") }
    func testChipGreen() throws { color = "rgb-0,1,0"; try captureFloating("chip") }
    func testChipBlue() throws { color = "rgb-0,0,1"; try captureFloating("chip") }
}

@MainActor final class NativeFloatingRainbowLightTests: FloatingControlTests {
    override var backdrop: String { "checkerboard" }
    func testChipWithoutLocal() throws { try captureFloating("chip", probe: "floating-local") }
    func testChipWithoutGlobal() throws { try captureFloating("chip", probe: "floating-global") }
    func testButtonWithoutLocal() throws { try captureFloating("button", probe: "floating-local") }
    func testButtonWithoutGlobal() throws { try captureFloating("button", probe: "floating-global") }
}

@MainActor final class NativeFloatingRainbowBlurTests: FloatingControlTests {
    override var backdrop: String { "checkerboard" }
    func testChipWithoutFillBlur() throws { try captureFloating("chip", probe: "fill-blur") }
    func testChipHalfFillBlur() throws { try captureFloating("chip", probe: "fill-blur-half") }
    func testButtonWithoutFillBlur() throws { try captureFloating("button", probe: "fill-blur") }
    func testButtonHalfFillBlur() throws { try captureFloating("button", probe: "fill-blur-half") }
}

@MainActor class NativeFloatingPositionTests: FloatingControlTests {
    func testLeftContact() throws { try captureFloating("button", pressOffset: -24) }
    func testRightContact() throws { try captureFloating("button", pressOffset: 24) }
}

@MainActor final class CranposeFloatingPositionTests: NativeFloatingPositionTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor class NativeFloatingTrackingTests: ReferenceUITests {
    func testReleaseTolerance() throws {
        for (component, title) in [("chip", "Unread"), ("button", "Continue"), ("icon-button", "Search")] {
            let distances: [CGFloat] = component == "icon-button" ? [50, 60] : [69, 70]
            for beyondEdge in distances {
                let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
                app.launchEnvironment = ["REFERENCE_COMPONENT": component, "REFERENCE_BACKDROP": "checkerboard-mono"]
                app.launch()
                XCTAssertTrue(app.staticTexts["Reference control"].waitForExistence(timeout: 10))
                let frame = app.buttons[title].firstMatch.frame
                XCTAssertGreaterThan(frame.width, 0)
                let point = CGPoint(x: frame.midX, y: frame.midY)
                _ = try ReferenceGesture.synthesize(
                    points: [NSValue(cgPoint: point), NSValue(cgPoint: point),
                             NSValue(cgPoint: CGPoint(x: frame.maxX + beyondEdge, y: frame.midY)),
                             NSValue(cgPoint: CGPoint(x: frame.maxX + beyondEdge, y: frame.midY))],
                    offsets: [0, 0.3, 0.6, 0.9], name: "Floating release tolerance")
                capture("\(component)-release-\(beyondEdge)", app: app)
                let activated = app.staticTexts["Activations: 1"].exists
                print("floating-tracking component=\(component) edge=\(beyondEdge) frame=\(frame) activated=\(activated)")
                XCTAssertEqual(activated, beyondEdge == distances[0])
            }
        }
    }
}

@MainActor final class CranposeFloatingTrackingTests: NativeFloatingTrackingTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class NativeFloatingRainbowTests: NativeFloatingMonoTests {
    override var backdrop: String { "checkerboard" }
}

@MainActor final class CranposeFloatingMonoTests: NativeFloatingMonoTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor final class CranposeFloatingRainbowTests: NativeFloatingMonoTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
    override var backdrop: String { "checkerboard" }
}

@MainActor final class NativeFloatingDarkRainbowTests: NativeFloatingMonoTests {
    override var backdrop: String { "checkerboard-dark-rainbow" }
}

@MainActor final class CranposeFloatingDarkRainbowTests: NativeFloatingMonoTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
    override var backdrop: String { "checkerboard-dark-rainbow" }
}

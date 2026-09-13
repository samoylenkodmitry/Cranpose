import XCTest

@MainActor
class TabBarTests: XCTestCase {
    var bundleIdentifier: String { "io.cranpose.liquid-reference" }

    func launch(scheme: String = "light", backdrop: String = "checkerboard", initial: Int = 0, settlingSeconds: Double = 0.8,
                contactFilterTime: Double? = nil, materialProfile: Int? = nil,
                content: String? = nil, firstTitle: String = "Discover") -> XCUIApplication {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: bundleIdentifier)
        app.launchEnvironment["REFERENCE_SCHEME"] = scheme
        app.launchEnvironment["REFERENCE_BACKDROP"] = backdrop
        app.launchEnvironment["REFERENCE_RECORDING"] = "1"
        app.launchEnvironment["REFERENCE_INITIAL_DESTINATION"] = String(initial)
        app.launchEnvironment["REFERENCE_SETTLING_SECONDS"] = String(settlingSeconds)
        if let contactFilterTime { app.launchEnvironment["REFERENCE_CONTACT_FILTER_TIME"] = String(contactFilterTime) }
        if let materialProfile { app.launchEnvironment["REFERENCE_MATERIAL_PROFILE"] = String(materialProfile) }
        if let content { app.launchEnvironment["REFERENCE_CONTENT"] = content }
        app.launch()
        XCTAssertTrue(app.buttons[firstTitle].waitForExistence(timeout: 10))
        return app
    }

    func capture(_ name: String, app: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: app.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        let tree = XCTAttachment(string: app.debugDescription)
        tree.name = name + "-accessibility"
        tree.lifetime = .keepAlways
        add(tree)
    }

    func assertDestination(_ title: String, app: XCUIApplication) {
        let destination = app.staticTexts[title].firstMatch
        XCTAssertTrue(destination.exists)
        XCTAssertLessThan(destination.frame.midY, app.frame.height * 0.75)
        XCTAssertTrue(app.buttons[title].isSelected)
    }

    func testTapAndRetap() {
        let app = launch()
        capture("light-checkerboard-rest", app: app)
        for title in ["Account", "Browse", "Saved", "Discover", "Discover"] {
            app.buttons[title].tap()
            assertDestination(title, app: app)
            capture("tap-" + title, app: app)
        }
    }

    func testNativeCellGeometry() {
        let app = launch(backdrop: "solid")
        let width = app.frame.width - 50
        let cellWidth = width / 4 + 7
        let pitch = (width - cellWidth) / 3
        for (index, title) in ["Discover", "Browse", "Saved", "Account"].enumerated() {
            let frame = app.buttons[title].frame
            XCTAssertEqual(frame.width, cellWidth, accuracy: 0.34)
            XCTAssertEqual(frame.midX, 25 + cellWidth / 2 + CGFloat(index) * pitch, accuracy: 0.34)
            XCTAssertEqual(frame.height, 54, accuracy: 0.34)
            XCTAssertEqual(frame.midY, app.frame.height - 52, accuracy: 0.34)
        }
        capture("native-cell-geometry", app: app)
    }

    func testNativeCaptionWidths() throws {
        let app = launch(backdrop: "solid")
        let image = try XCTUnwrap(app.screenshot().image.cgImage)
        let data = try XCTUnwrap(image.dataProvider?.data) as Data
        XCTAssertEqual(image.bitsPerComponent, 8)
        XCTAssertTrue([24, 32].contains(image.bitsPerPixel))
        let stride = image.bitsPerPixel / 8
        let firstAlpha = [CGImageAlphaInfo.first, .premultipliedFirst, .noneSkipFirst].contains(image.alphaInfo)
        let little = image.bitmapInfo.contains(.byteOrder32Little)
        let colorStart = stride == 4 && firstAlpha != little ? 1 : 0
        let scale = CGFloat(image.width) / app.frame.width
        let top = Int((app.frame.height - 43) * scale)
        let bottom = Int((app.frame.height - 31) * scale)
        for (title, expected) in [("Browse", 34.333), ("Account", 39.333)] {
            let center = app.buttons[title].frame.midX
            let left = Int((center - 30) * scale)
            let right = Int((center + 30) * scale)
            var minimum = right
            var maximum = left
            for y in top..<bottom {
                for x in left..<right {
                    let offset = y * image.bytesPerRow + x * stride + colorStart
                    if (0..<3).allSatisfy({ data[offset + $0] < 70 }) {
                        minimum = min(minimum, x)
                        maximum = max(maximum, x)
                    }
                }
            }
            XCTAssertGreaterThan(maximum, minimum, "\(title) ink must be present")
            XCTAssertEqual(Double(maximum - minimum + 1) / Double(scale), expected, accuracy: 0.34, title)
        }
        capture("native-caption-widths", app: app)
    }

    func testScrubAcrossTabs() {
        let app = launch()
        let first = app.buttons["Discover"]
        let last = app.buttons["Account"]
        first.press(forDuration: 0.5, thenDragTo: last, withVelocity: .slow, thenHoldForDuration: 0.5)
        assertDestination("Account", app: app)
        capture("scrub-forward-released", app: app)
        last.press(forDuration: 0.5, thenDragTo: first, withVelocity: .slow, thenHoldForDuration: 0.5)
        assertDestination("Discover", app: app)
        capture("scrub-back-released", app: app)
    }

    func testDragOutside() {
        let app = launch()
        let first = app.buttons["Discover"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let outside = app.buttons["Account"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .withOffset(CGVector(dx: 0, dy: -180))
        first.press(forDuration: 0.5, thenDragTo: outside, withVelocity: .slow, thenHoldForDuration: 0.5)
        capture("outside-release", app: app)
        assertDestination("Account", app: app)
    }

    func testHoldUnselected() {
        let app = launch()
        app.buttons["Account"].press(forDuration: 3)
        assertDestination("Account", app: app)
        capture("held-unselected-released", app: app)
    }

    func testInteractionKeyframes() {
        let app = launch()
        continueAfterFailure = true
        capture("gesture-rest", app: app)
        let viewport = XCTAttachment(string: "{\"width\":\(app.frame.width),\"height\":\(app.frame.height)}")
        viewport.name = "gesture-viewport"
        viewport.lifetime = .keepAlways
        add(viewport)
        for (source, destination, speed) in [("Discover", "Account", 250.0), ("Account", "Discover", 1000.0),
                                             ("Discover", "Account", 1000.0), ("Account", "Discover", 250.0)] {
            XCTContext.runActivity(named: "down-0.8s-slide-\(speed)ptps-stop-idle-1.2s-up") { activity in
                let start = app.buttons[source].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                let end = app.buttons[destination].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                start.press(forDuration: 0.8, thenDragTo: end,
                            withVelocity: XCUIGestureVelocity(rawValue: speed), thenHoldForDuration: 1.2)
                Thread.sleep(forTimeInterval: 0.8)
                capture("gesture-\(speed)-released", app: app)
                assertDestination(destination, app: app)
                XCTAssertFalse(app.staticTexts["trace-error"].exists)
            }
        }
    }

    func testSurfaces() {
        for scheme in ["light", "dark"] {
            for backdrop in ["solid", "checkerboard"] {
                let app = launch(scheme: scheme, backdrop: backdrop)
                capture(scheme + "-" + backdrop, app: app)
                app.terminate()
            }
        }
    }
}

@MainActor
final class CranposeTabBarTests: TabBarTests {
    override var bundleIdentifier: String { "io.cranpose.liquid-cranpose" }
}

@MainActor
final class NativeContactFilterTests: XCTestCase {
    private func capture(at time: Double, drag: Bool = false) {
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.liquid-reference")
        app.launchEnvironment["REFERENCE_CONTACT_FILTER_TIME"] = String(time)
        app.launch()
        XCTAssertTrue(app.buttons["Discover"].waitForExistence(timeout: 10))
        if drag {
            app.buttons["Discover"].press(forDuration: 0.8, thenDragTo: app.buttons["Account"],
                                          withVelocity: XCUIGestureVelocity(rawValue: 1000), thenHoldForDuration: 1.2)
        } else {
            app.buttons["Discover"].press(forDuration: time + 0.4)
        }
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
    }

    func test025() { capture(at: 0.025) }
    func test050() { capture(at: 0.050) }
    func test075() { capture(at: 0.075) }
    func test100() { capture(at: 0.100) }
    func test150() { capture(at: 0.150) }
    func test200() { capture(at: 0.200) }
    func test300() { capture(at: 0.300) }
    func test500() { capture(at: 0.500) }
    func test1000() { capture(at: 1.000) }
    func testFastDrag() { capture(at: 0.980, drag: true) }
    func testFastRebound() { capture(at: 1.400, drag: true) }
}

@MainActor
final class NativeContentTests: XCTestCase {
    func testCaptureVibrancyBackdrop() {
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.liquid-reference")
        app.launchEnvironment["REFERENCE_EXPORT_CONTENT"] = "1"
        app.launchEnvironment["REFERENCE_HIDE_CONTENT"] = "1"
        app.launch()
        XCTAssertTrue(app.buttons["Discover"].waitForExistence(timeout: 10))
        let before = XCTAttachment(screenshot: app.screenshot())
        before.name = "vibrancy-content"
        before.lifetime = .keepAlways
        add(before)
        app.buttons["Discover"].tap()
        let settled = NSPredicate(format: "label == %@", "Settled · trace saved")
        expectation(for: settled, evaluatedWith: app.staticTexts["touch-phase"])
        waitForExpectations(timeout: 10)
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
        let after = XCTAttachment(screenshot: app.screenshot())
        after.name = "vibrancy-backdrop"
        after.lifetime = .keepAlways
        add(after)
    }

    func testExportNativeContent() {
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.liquid-reference")
        app.launchEnvironment["REFERENCE_EXPORT_CONTENT"] = "1"
        app.launch()
        XCTAssertTrue(app.buttons["Discover"].waitForExistence(timeout: 10))
        app.buttons["Discover"].tap()
        XCTAssertFalse(app.staticTexts["trace-error"].exists)
    }
}

@MainActor
class OpticalProbeTests: XCTestCase {
    var scheme: String { "light" }
    var wavePeriod: Double { 128 }
    var isolatesChannels: Bool { false }
    var isolatesLayers: Bool { false }
    var exposesSDF: Bool { false }
    var isolatesMonochrome: Bool { false }
    var count: Int { isolatesChannels ? 24 : isolatesLayers ? 12 : exposesSDF ? 18 : isolatesMonochrome ? 8 : 35 }

    func capture(_ indices: Range<Int>, kernel: String = "none") {
        let count: Int
        switch kernel {
        case "none": count = self.count
        case "contact-step", "pane-image": count = 10
        case "adaptive-flat": count = 7
        case "background-opacity": count = 6
        case "resting-blur": count = 4
        case "glow-mask", "lens-shadow", "inner-shadow": count = 2
        default: count = 8
        }
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.liquid-reference")
        app.launchEnvironment["REFERENCE_SCHEME"] = scheme
        app.launchEnvironment["REFERENCE_OPTICAL_PROBES"] = "1"
        app.launchEnvironment["REFERENCE_OPTICAL_INDEX"] = String(indices.lowerBound)
        app.launchEnvironment["REFERENCE_OPTICAL_CHANNELS"] = isolatesChannels ? "1" : "0"
        app.launchEnvironment["REFERENCE_OPTICAL_LAYERS"] = isolatesLayers ? "1" : "0"
        app.launchEnvironment["REFERENCE_OPTICAL_SDF"] = exposesSDF ? "1" : "0"
        app.launchEnvironment["REFERENCE_OPTICAL_MONOCHROME"] = isolatesMonochrome ? "1" : "0"
        app.launchEnvironment["REFERENCE_OPTICAL_KERNEL"] = kernel
        app.launchEnvironment["REFERENCE_OPTICAL_PERIOD"] = String(wavePeriod)
        app.launchEnvironment["REFERENCE_EXPORT_CONTENT"] = "1"
        app.launchEnvironment["REFERENCE_HIDE_CONTENT"] = "1"
        app.launch()
        XCTAssertTrue(app.buttons["Account"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["optical-probe"].waitForExistence(timeout: 5))
        for index in indices {
            if index != indices.lowerBound {
                app.buttons["Discover"].tap()
                app.buttons["Next optical probe"].tap()
            }
            XCTAssertEqual(app.staticTexts["optical-probe"].label, "Optical probe \(index + 1)/\(count) ready")
            Thread.sleep(forTimeInterval: 0.8)
            app.buttons["Account"].press(forDuration: 3.5)
            XCTAssertEqual(app.staticTexts["optical-probe"].label, "Optical probe \(index + 1)/\(count) captured")
            XCTAssertFalse(app.staticTexts["trace-error"].exists)
            Thread.sleep(forTimeInterval: 2.0)
            let pixels = XCTAttachment(screenshot: app.screenshot())
            pixels.name = "optical-probe-\(index)"
            pixels.lifetime = .keepAlways
            add(pixels)
        }
    }

}

@MainActor
final class NativeRestingBlurTests: OpticalProbeTests {
    func testKernel() {
        for index in 0..<4 { capture(index..<(index + 1), kernel: "resting-blur") }
    }
}

@MainActor
final class NativeOpticsTests: OpticalProbeTests {
    func testProbeGroup0() { capture(0..<5) }
    func testProbeGroup1() { capture(5..<10) }
    func testProbeGroup2() { capture(10..<15) }
    func testProbeGroup3() { capture(15..<20) }
    func testProbeGroup4() { capture(20..<25) }
    func testProbeGroup5() { capture(25..<30) }
    func testProbeGroup6() { capture(30..<35) }
}

@MainActor
final class NativeMonochromeOpticsTests: OpticalProbeTests {
    override var isolatesMonochrome: Bool { true }

    func testHorizontal() { capture(0..<4) }
    func testVertical() { capture(4..<8) }
}

@MainActor
final class NativeBackgroundOpacityTests: OpticalProbeTests {
    func testOpacity() { capture(0..<6, kernel: "background-opacity") }
}

@MainActor
final class NativeKernelOpticsTests: OpticalProbeTests {
    func testForegroundX0() { capture(0..<1, kernel: "foreground-wave") }
    func testForegroundX1() { capture(1..<2, kernel: "foreground-wave") }
    func testForegroundX2() { capture(2..<3, kernel: "foreground-wave") }
    func testForegroundX3() { capture(3..<4, kernel: "foreground-wave") }
    func testForegroundY0() { capture(4..<5, kernel: "foreground-wave") }
    func testForegroundY1() { capture(5..<6, kernel: "foreground-wave") }
    func testForegroundY2() { capture(6..<7, kernel: "foreground-wave") }
    func testForegroundY3() { capture(7..<8, kernel: "foreground-wave") }
    func testSpectralStepX0() { capture(0..<1, kernel: "foreground-step") }
    func testSpectralStepX1() { capture(1..<2, kernel: "foreground-step") }
    func testSpectralStepX2() { capture(2..<3, kernel: "foreground-step") }
    func testSpectralStepX3() { capture(3..<4, kernel: "foreground-step") }
    func testSpectralStepY0() { capture(4..<5, kernel: "foreground-step") }
    func testSpectralStepY1() { capture(5..<6, kernel: "foreground-step") }
    func testSpectralStepY2() { capture(6..<7, kernel: "foreground-step") }
    func testSpectralStepY3() { capture(7..<8, kernel: "foreground-step") }
    func testBackgroundX0() { capture(0..<1, kernel: "background-wave") }
    func testBackgroundX1() { capture(1..<2, kernel: "background-wave") }
    func testBackgroundX2() { capture(2..<3, kernel: "background-wave") }
    func testBackgroundX3() { capture(3..<4, kernel: "background-wave") }
    func testBackgroundY0() { capture(4..<5, kernel: "background-wave") }
    func testBackgroundY1() { capture(5..<6, kernel: "background-wave") }
    func testBackgroundY2() { capture(6..<7, kernel: "background-wave") }
    func testBackgroundY3() { capture(7..<8, kernel: "background-wave") }
}

@MainActor
final class NativeInnerShadowOpticsTests: OpticalProbeTests {
    func test0() { capture(0..<1, kernel: "inner-shadow") }
    func test1() { capture(1..<2, kernel: "inner-shadow") }
}

@MainActor
class NativeAdaptiveOpticsTests: OpticalProbeTests {
    func test0() { capture(0..<1, kernel: "adaptive-flat") }
    func test1() { capture(1..<2, kernel: "adaptive-flat") }
    func test2() { capture(2..<3, kernel: "adaptive-flat") }
    func test3() { capture(3..<4, kernel: "adaptive-flat") }
    func test4() { capture(4..<5, kernel: "adaptive-flat") }
    func test5() { capture(5..<6, kernel: "adaptive-flat") }
    func test6() { capture(6..<7, kernel: "adaptive-flat") }
}

@MainActor
final class NativeDarkAdaptiveOpticsTests: NativeAdaptiveOpticsTests {
    override var scheme: String { "dark" }
}

@MainActor
class NativeAdaptiveBoundaryTests: OpticalProbeTests {
    var boundaryKernel: String { "adaptive-boundaries" }
    func testLower() { capture(0..<4, kernel: boundaryKernel) }
    func testUpper() { capture(4..<8, kernel: boundaryKernel) }
}

@MainActor
final class NativeDarkAdaptiveBoundaryTests: NativeAdaptiveBoundaryTests {
    override var scheme: String { "dark" }
}

@MainActor
class NativeFineAdaptiveBoundaryTests: NativeAdaptiveBoundaryTests {
    override var boundaryKernel: String { "adaptive-boundaries-fine" }
}

@MainActor
final class NativeDarkFineAdaptiveBoundaryTests: NativeFineAdaptiveBoundaryTests {
    override var scheme: String { "dark" }
}

@MainActor
final class NativeContactKernelOpticsTests: OpticalProbeTests {
    func testX000() { capture(0..<1, kernel: "contact-step") }
    func testY000() { capture(1..<2, kernel: "contact-step") }
    func testX025() { capture(2..<3, kernel: "contact-step") }
    func testY025() { capture(3..<4, kernel: "contact-step") }
    func testX050() { capture(4..<5, kernel: "contact-step") }
    func testY050() { capture(5..<6, kernel: "contact-step") }
    func testX075() { capture(6..<7, kernel: "contact-step") }
    func testY075() { capture(7..<8, kernel: "contact-step") }
    func testX100() { capture(8..<9, kernel: "contact-step") }
    func testY100() { capture(9..<10, kernel: "contact-step") }
}

@MainActor
class NativePaneKernelOpticsTests: OpticalProbeTests {
    func testX0() { capture(0..<1, kernel: "pane-wave") }
    func testX1() { capture(1..<2, kernel: "pane-wave") }
    func testX2() { capture(2..<3, kernel: "pane-wave") }
    func testX3() { capture(3..<4, kernel: "pane-wave") }
    func testY0() { capture(4..<5, kernel: "pane-wave") }
    func testY1() { capture(5..<6, kernel: "pane-wave") }
    func testY2() { capture(6..<7, kernel: "pane-wave") }
    func testY3() { capture(7..<8, kernel: "pane-wave") }
}

@MainActor
final class NativePaneFineKernelOpticsTests: NativePaneKernelOpticsTests {
    override var wavePeriod: Double { 32 }
}

@MainActor
final class NativeContentKernelOpticsTests: OpticalProbeTests {
    func testX0() { capture(0..<1, kernel: "content-wave") }
    func testX1() { capture(1..<2, kernel: "content-wave") }
    func testX2() { capture(2..<3, kernel: "content-wave") }
    func testX3() { capture(3..<4, kernel: "content-wave") }
    func testY0() { capture(4..<5, kernel: "content-wave") }
    func testY1() { capture(5..<6, kernel: "content-wave") }
    func testY2() { capture(6..<7, kernel: "content-wave") }
    func testY3() { capture(7..<8, kernel: "content-wave") }
}

@MainActor
final class NativeChannelOpticsTests: OpticalProbeTests {
    override var isolatesChannels: Bool { true }

    func testRedHorizontal() { capture(0..<4) }
    func testRedVertical() { capture(4..<8) }
    func testGreenHorizontal() { capture(8..<12) }
    func testGreenVertical() { capture(12..<16) }
    func testBlueHorizontal() { capture(16..<20) }
    func testBlueVertical() { capture(20..<24) }
}

@MainActor
final class NativeLayerOpticsTests: OpticalProbeTests {
    override var isolatesLayers: Bool { true }

    func testBaseline() { capture(0..<1) }
    func testInitialWarp() { capture(1..<2) }
    func testLiftedWarp() { capture(2..<3) }
    func testContentWarp() { capture(3..<4) }
    func testChromaticForeground() { capture(4..<5) }
    func testHighlight() { capture(5..<6) }
    func testGlobalGlow() { capture(6..<7) }
    func testLocalGlow() { capture(7..<8) }
    func testLensBackgroundWarp() { capture(8..<9) }
    func testPaneHighlight() { capture(9..<10) }
    func testPaneHighlightAndGlobal() { capture(10..<11) }
    func testInnerShadow() { capture(11..<12) }
}

@MainActor
final class NativeSDFOpticsTests: OpticalProbeTests {
    override var exposesSDF: Bool { true }

    func testInitialBlack() { capture(0..<1) }
    func testInitialWhite() { capture(1..<2) }
    func testLiftedBlack() { capture(2..<3) }
    func testLiftedWhite() { capture(3..<4) }
    func testContentBlack() { capture(4..<5) }
    func testContentWhite() { capture(5..<6) }
    func testHighlightBlack() { capture(6..<7) }
    func testHighlightWhite() { capture(7..<8) }
    func testBackgroundBlack() { capture(8..<9) }
    func testBackgroundWhite() { capture(9..<10) }
    func testForegroundBlack() { capture(10..<11) }
    func testForegroundWhite() { capture(11..<12) }
    func testPaneHighlightBlack() { capture(12..<13) }
    func testPaneHighlightWhite() { capture(13..<14) }
    func testPaneHighlightTallBlack() { capture(14..<15) }
    func testPaneHighlightTallWhite() { capture(15..<16) }
    func testLensHighlightTallBlack() { capture(16..<17) }
    func testLensHighlightTallWhite() { capture(17..<18) }
}

@MainActor
final class NativeGlowMaskTests: OpticalProbeTests {
    func testContactOpacity() { capture(0..<1, kernel: "glow-mask") }
    func testUnitOpacity() { capture(1..<2, kernel: "glow-mask") }
}

@MainActor
final class NativeLensShadowOpticsTests: OpticalProbeTests {
    func testBaseline() { capture(0..<1, kernel: "lens-shadow") }
    func testWithoutShadow() { capture(1..<2, kernel: "lens-shadow") }
}

@MainActor
final class NativePaneImageOpticsTests: OpticalProbeTests {
    func test0() { capture(0..<1, kernel: "pane-image") }
    func test1() { capture(1..<2, kernel: "pane-image") }
    func test2() { capture(2..<3, kernel: "pane-image") }
    func test3() { capture(3..<4, kernel: "pane-image") }
    func test4() { capture(4..<5, kernel: "pane-image") }
    func test5() { capture(5..<6, kernel: "pane-image") }
    func test6() { capture(6..<7, kernel: "pane-image") }
    func test7() { capture(7..<8, kernel: "pane-image") }
    func test8() { capture(8..<9, kernel: "pane-image") }
    func test9() { capture(9..<10, kernel: "pane-image") }
}

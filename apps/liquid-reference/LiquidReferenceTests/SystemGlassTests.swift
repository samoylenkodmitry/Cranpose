import XCTest

@MainActor
extension TabBarTests {
    func systemGlassSettings() -> XCUIApplication {
        let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
        settings.launch()
        let search = settings.searchFields["Search"]
        XCTAssertTrue(search.waitForExistence(timeout: 5))
        search.tap()
        search.typeText("Liquid Glass")
        let entry = settings.buttons["OpenAppearanceDeepLink.AppearanceDeepLink./LIQUID_GLASS"]
        XCTAssertTrue(entry.waitForExistence(timeout: 10))
        entry.tap()
        XCTAssertTrue(settings.sliders["Tint Amount"].waitForExistence(timeout: 5))
        return settings
    }

    func setSystemGlassTint(_ percent: Int) {
        XCTAssertTrue((0...100).contains(percent))
        let settings = systemGlassSettings()
        let slider = settings.sliders["Tint Amount"]
        var position = CGFloat(percent) / 100
        for _ in 0..<18 {
            guard let value = slider.value as? String,
                  let actual = Int(value.replacingOccurrences(of: "%", with: "")) else {
                XCTFail("Tint Amount must expose a numeric percentage")
                return
            }
            print("System glass tint requested \(percent), measured \(actual), endpoint \(position)")
            if actual == percent { break }
            let inset = slider.frame.height * 0.5
            let trackWidth = slider.frame.width - 2 * inset
            let start = CGPoint(x: slider.frame.minX + inset + trackWidth * CGFloat(actual) / 100,
                                y: slider.frame.midY)
            let end = CGPoint(x: slider.frame.minX + inset + trackWidth * position, y: start.y)
            do {
                let direction: CGFloat = position < 0.5 ? 1 : -1
                let excursion = CGPoint(x: start.x + direction * trackWidth * 0.1, y: start.y)
                _ = try ReferenceGesture.synthesize(points: [start, start, excursion, end, end].map { NSValue(cgPoint: $0) },
                                                    offsets: [0, 0.6, 0.9, 1.2, 1.7], name: "Set system glass tint")
            } catch {
                XCTFail("System tint gesture failed: \(error)")
                return
            }
            Thread.sleep(forTimeInterval: 0.5)
            if let value = slider.value as? String,
               let measured = Int(value.replacingOccurrences(of: "%", with: "")) {
                position = min(1.05, max(-0.05, position + CGFloat(percent - measured) / 200))
            }

        }
        XCTAssertEqual(slider.value as? String, "\(percent)%")
        capture("system-glass-tint-\(percent)", app: settings)
    }
}

@MainActor
final class SystemGlassTests: TabBarTests {
    func testInspectSettings() {
        capture("system-glass-settings", app: systemGlassSettings())
    }
}

@MainActor
final class NativeMaterialProfileTests: TabBarTests {
    private func captureProfile(_ percent: Int) {
        continueAfterFailure = false
        addTeardownBlock { @MainActor [self] in
            continueAfterFailure = true
            setSystemGlassTint(25)
        }
        do {
            setSystemGlassTint(percent)
            let app = launch(initial: 3, contactFilterTime: 0.5, materialProfile: percent)
            app.buttons["Account"].press(forDuration: 1.0)
            XCTAssertFalse(app.staticTexts["trace-error"].exists)
            capture("native-material-\(percent)", app: app)
        }
    }

    func testTint0() { captureProfile(0) }
    func testTint25() { captureProfile(25) }
    func testTint50() { captureProfile(50) }
    func testTint100() { captureProfile(100) }
}

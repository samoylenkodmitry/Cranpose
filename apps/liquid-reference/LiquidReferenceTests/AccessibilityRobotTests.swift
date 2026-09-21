import XCTest

@MainActor
final class AccessibilityRobotTests: XCTestCase {
    private func launch() -> XCUIApplication {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.demo")
        app.launchArguments = ["--test_screen=accessibility_robot"]
        app.launch()
        XCTAssertTrue(named("Accessibility robot", in: app).waitForExistence(timeout: 15))
        return app
    }

    private func named(_ name: String, in app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", name)).firstMatch
    }

    private func capture(_ app: XCUIApplication, _ name: String) {
        for attachment in [XCTAttachment(screenshot: app.screenshot()), XCTAttachment(string: app.debugDescription)] {
            attachment.name = name
            attachment.lifetime = .keepAlways
            add(attachment)
        }
    }

    func testNamesStateAndRanges() {
        let app = launch()
        defer { capture(app, "names-state-ranges") }
        XCTAssertTrue(named("Account, Account", in: app).exists)
        XCTAssertTrue(app.buttons["Remove"].exists)
        XCTAssertFalse(app.buttons["Disabled action"].isEnabled)
        XCTAssertFalse(app.debugDescription.contains("robot-secret-value"))
        XCTAssertFalse(named("Decorative secret", in: app).exists)
        let progress = named("Loading", in: app)
        XCTAssertTrue(progress.exists)
        XCTAssertFalse(app.sliders["Loading"].exists)
        XCTAssertEqual(progress.value as? String, "40")
        let volume = named("Volume", in: app)
        XCTAssertTrue(volume.exists)
        XCTAssertEqual(volume.value as? String, "30")
    }

    func testActivationThroughNativeControls() {
        let app = launch()
        defer { capture(app, "activation") }
        for (name, count) in [("Increase", 1), ("Remove", 2), ("Decrease", 1)] {
            app.buttons[name].tap()
            XCTAssertTrue(named("Action count: \(count)", in: app).waitForExistence(timeout: 5))
        }
    }

    func testNativeTextEditing() {
        let app = launch()
        defer { capture(app, "native-text-editing") }
        let notes = named("Notes", in: app)
        XCTAssertTrue(notes.exists)
        notes.tap()
        notes.typeText(" robot")
        XCTAssertTrue(named("Edited: initial robot", in: app).waitForExistence(timeout: 5))
    }
}

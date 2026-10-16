import UIKit
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

    /// The screen below the status bar, whose clock changes, as PNG data.
    private func content(_ app: XCUIApplication) -> Data? {
        guard let image = app.screenshot().image.cgImage else { return nil }
        let top = image.height / 8
        let below = CGRect(x: 0, y: top, width: image.width, height: image.height - top)
        return image.cropping(to: below).flatMap { UIImage(cgImage: $0).pngData() }
    }

    /// Whether the app draws a screen other than `previous` within five seconds.
    private func drawsChange(from previous: Data?, in app: XCUIApplication) -> Bool {
        let deadline = Date().addingTimeInterval(5)
        repeat {
            if content(app) != previous { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        return false
    }

    func testBackgroundAndReturn() {
        let app = launch()
        defer { capture(app, "background-return") }
        app.buttons["Increase"].tap()
        XCTAssertTrue(named("Action count: 1", in: app).waitForExistence(timeout: 5))
        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackgroundSuspended, timeout: 10) || app.state == .runningBackground)
        app.activate()
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        // The composition kept its state, and its input draws new frames.
        XCTAssertTrue(named("Action count: 1", in: app).waitForExistence(timeout: 5))
        let returned = content(app)
        app.buttons["Increase"].tap()
        XCTAssertTrue(named("Action count: 2", in: app).waitForExistence(timeout: 5))
        XCTAssertTrue(drawsChange(from: returned, in: app))
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
        XCTAssertEqual(named("Express", in: app).value as? String, "checked, 2 of 3")
        XCTAssertEqual(named("Standard", in: app).value as? String, "not checked, 1 of 3")
    }

    func testActivationThroughNativeControls() {
        let app = launch()
        defer { capture(app, "activation") }
        for (name, count) in [("Increase", 1), ("Remove", 2), ("Decrease", 1)] {
            app.buttons[name].tap()
            XCTAssertTrue(named("Action count: \(count)", in: app).waitForExistence(timeout: 5))
        }
        app.buttons["Open preferences"].tap()
        XCTAssertTrue(app.buttons["Close preferences"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Increase"].exists)
        app.buttons["Open confirmation"].tap()
        XCTAssertTrue(app.buttons["Close confirmation"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["Close preferences"].exists)
        app.buttons["Close confirmation"].tap()
        XCTAssertTrue(app.buttons["Close preferences"].waitForExistence(timeout: 5))
        app.buttons["Close preferences"].tap()
        XCTAssertTrue(app.buttons["Increase"].waitForExistence(timeout: 5))
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

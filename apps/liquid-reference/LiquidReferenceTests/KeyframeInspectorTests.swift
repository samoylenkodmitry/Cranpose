import XCTest

@MainActor
final class KeyframeInspectorTests: XCTestCase {
    func testBundledFramesCanBeSteppedAndPlayed() {
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.liquid-reference")
        app.launch()
        let inspect = app.buttons["Inspect animation keyframes"]
        XCTAssertTrue(inspect.waitForExistence(timeout: 10))
        inspect.tap()
        let counter = app.staticTexts["frame-counter"]
        XCTAssertTrue(counter.waitForExistence(timeout: 5))
        app.buttons["After down"].tap()
        XCTAssertTrue(app.images["Native iOS recorded frame"].exists)
        XCTAssertTrue(app.images["Cranpose recorded frame"].exists)
        let routes = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "recorded-route-")).allElementsBoundByIndex
        XCTAssertFalse(routes.isEmpty)
        for route in routes {
            route.tap()
            app.buttons["After down"].tap()
            XCTAssertTrue(app.images["Native iOS recorded frame"].exists)
            XCTAssertTrue(app.images["Cranpose recorded frame"].exists)
        }
        let first = counter.label
        app.buttons["Next frame"].tap()
        XCTAssertNotEqual(counter.label, first)
        for boundary in ["Before down", "Before up"] {
            app.buttons[boundary].tap()
            let before = counter.label
            app.buttons["Next frame"].tap()
            XCTAssertNotEqual(counter.label, before)
        }
        app.buttons["sliding"].tap()
        XCTAssertTrue(app.staticTexts["Native iOS · sliding"].exists)
        let sliding = counter.label
        app.buttons["Play"].tap()
        Thread.sleep(forTimeInterval: 0.2)
        XCTAssertNotEqual(counter.label, sliding)
        app.buttons["Pause"].tap()
        let screenshot = XCTAttachment(screenshot: app.screenshot())
        screenshot.name = "bundled-frame-inspector"
        screenshot.lifetime = .keepAlways
        add(screenshot)
    }
}

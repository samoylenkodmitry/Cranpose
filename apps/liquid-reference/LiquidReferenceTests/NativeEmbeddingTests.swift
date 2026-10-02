import XCTest
import UIKit

@MainActor
final class NativeEmbeddingTests: XCTestCase {
    func testNativeAndCranposeViewsExchangeEvents() {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: "dev.cranpose.nativehost")
        app.launchArguments = ["--native-component"]
        app.launch()
        defer {
            let attachment = XCTAttachment(screenshot: app.screenshot())
            attachment.name = "native-embedding"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
        let count = app.staticTexts["host-count"]
        XCTAssertTrue(count.waitForExistence(timeout: 20))
        let ready = NSPredicate(format: "label == %@", "Host received: 0")
        expectation(for: ready, evaluatedWith: count)
        waitForExpectations(timeout: 20)
        app.buttons["native-increment"].tap()
        expectation(for: NSPredicate(format: "label == %@", "Host received: 1"), evaluatedWith: count)
        waitForExpectations(timeout: 10)
        let component = app.otherElements["cranpose-component"]
        XCTAssertTrue(component.exists)
        let screenshot = component.screenshot().image
        let sample = screenshot.cgImage?.cropping(to: CGRect(x: 8, y: 8, width: 1, height: 1))
        let data = sample?.dataProvider?.data
        if let data, let bytes = CFDataGetBytePtr(data) {
            XCTAssertGreaterThan(bytes[0], 230, "Cranpose background must be light")
            XCTAssertGreaterThan(bytes[1], 230)
            XCTAssertGreaterThan(bytes[2], 230)
        } else { XCTFail("Missing rendered component image") }
        component.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 60, dy: 96)).tap()
        expectation(for: NSPredicate(format: "label == %@", "Host received: 2"), evaluatedWith: count)
        waitForExpectations(timeout: 10)
        app.buttons["Cranpose → Native"].tap()
        let web = app.webViews.firstMatch
        XCTAssertTrue(web.waitForExistence(timeout: 20))
        XCTAssertTrue(web.links["Learn more"].waitForExistence(timeout: 30))
        app.buttons["native-increment"].tap()
        expectation(for: NSPredicate(format: "label == %@", "Host received: 1"), evaluatedWith: count)
        waitForExpectations(timeout: 10)
        XCTAssertTrue(web.links["Learn more"].exists)
        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertTrue(web.links["Learn more"].waitForExistence(timeout: 10))
        XCTAssertEqual(count.label, "Host received: 1")
        let host = app.otherElements["cranpose-component"]
        host.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 170, dy: 96)).tap()
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: web)
        waitForExpectations(timeout: 10)
        host.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 170, dy: 96)).tap()
        XCTAssertTrue(web.waitForExistence(timeout: 10))
        app.buttons["Native → Cranpose"].tap()
        XCTAssertFalse(app.webViews.firstMatch.exists)
    }
}

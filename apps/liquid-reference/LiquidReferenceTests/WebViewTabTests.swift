import XCTest

@MainActor
final class WebViewTabTests: XCTestCase {
    func testWebsiteSharesTheScreenWithCranpose() {
        continueAfterFailure = false
        let app = XCUIApplication(bundleIdentifier: "io.cranpose.demo")
        app.launchArguments = ["--tab=webview"]
        app.launch()
        defer {
            let attachment = XCTAttachment(screenshot: app.screenshot())
            attachment.lifetime = .keepAlways
            add(attachment)
        }
        let web = app.webViews.firstMatch
        XCTAssertTrue(web.waitForExistence(timeout: 20))
        XCTAssertTrue(web.links["Learn more"].waitForExistence(timeout: 30))
        app.buttons["Cranpose: 0"].tap()
        XCTAssertTrue(app.buttons["Cranpose: 1"].waitForExistence(timeout: 10))
        XCTAssertTrue(web.links["Learn more"].exists)
        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertTrue(web.links["Learn more"].waitForExistence(timeout: 10))
        app.buttons["Remove WebView"].tap()
        expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: web)
        waitForExpectations(timeout: 10)
        app.buttons["Show WebView"].tap()
        XCTAssertTrue(web.links["Learn more"].waitForExistence(timeout: 30))
    }
}

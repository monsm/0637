import XCTest

/// Taps the app's own controls in the simulator.
///
/// `ios/scripts/sim-drive.sh` proves the core through the intent API: the
/// walk policy, the skills, sit/stand, ground pick, push, reload and the
/// renderer. That leaves the UI layer itself untested - a button wired to
/// the wrong action would still pass. This test closes that gap: it reads
/// the label the status panel shows (fed from the same core status the
/// Python session asserts on) and taps each button the app offers.
final class DuckUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
    }

    /// The status panel's label text, identified so it can be read from here.
    private func labelElement() -> XCUIElement {
        app.staticTexts["duck-label"]
    }

    /// Poll the visible label until it is one of `wanted`, returning the last
    /// value seen. Polling rather than `waitForExistence` because the label
    /// changes while the element stays put.
    private func waitForLabel(_ wanted: [String], timeout: TimeInterval) -> String {
        let deadline = Date().addingTimeInterval(timeout)
        var last = ""
        while Date() < deadline {
            let element = labelElement()
            if element.exists {
                last = element.label
                if wanted.contains(last) { return last }
            }
            usleep(100_000)
        }
        return last
    }

    func testButtonsDriveTheCore() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading"
        )

        let ready = waitForLabel(["walk", "stand"], timeout: 180)
        XCTAssertTrue(
            ["walk", "stand"].contains(ready),
            "the 50 Hz loop never drove the duck: label=\(ready)"
        )

        // roulade is a 1 s one-shot: long enough to observe through XCUITest.
        app.buttons["roulade"].tap()
        XCTAssertEqual(
            waitForLabel(["roulade"], timeout: 20), "roulade",
            "the roulade button did not reach the core"
        )
        _ = waitForLabel(["walk", "stand"], timeout: 30)

        app.buttons["Ground pick"].tap()
        XCTAssertEqual(
            waitForLabel(["ground_pick"], timeout: 20), "ground_pick",
            "the ground pick button did not reach the core"
        )
        _ = waitForLabel(["walk", "stand"], timeout: 40)

        app.buttons["Sit / Stand"].tap()
        XCTAssertEqual(
            waitForLabel(["sit"], timeout: 20), "sit",
            "the sit button did not seat the duck"
        )
        // The prototype refuses the way back up during the 2 s seat settle.
        Thread.sleep(forTimeInterval: 3.5)
        app.buttons["Sit / Stand"].tap()
        let standing = waitForLabel(["rise", "walk", "stand"], timeout: 40)
        XCTAssertTrue(
            ["rise", "walk", "stand"].contains(standing),
            "the sit toggle did not stand the duck up: label=\(standing)"
        )

        // Reload reopens every ONNX session; the duck must still be driving.
        app.buttons["Reload AI"].tap()
        let afterReload = waitForLabel(["walk", "stand"], timeout: 90)
        XCTAssertTrue(
            ["walk", "stand"].contains(afterReload),
            "the duck stopped after reloading the policies: label=\(afterReload)"
        )
    }
}

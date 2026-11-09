import XCTest

/// Taps the app's own controls.
///
/// `ios/scripts/sim-drive.sh` proves the core through the intent API; this
/// proves the shell is wired to it. The interface is the live duck — the skill
/// ring and the time-machine sheet — so the test opens the ring, picks a
/// skill, and drives the sheet's buttons, asserting the label the status panel
/// shows (fed from the same core status the Python session asserts on).
final class DuckUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launch()
    }

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

    /// Open the skill ring and pick a skill by its visible name.
    private func pickSkill(_ name: String) throws {
        let skills = app.buttons["Skills"]
        XCTAssertTrue(skills.waitForExistence(timeout: 10), "the skills button is missing")
        skills.tap()
        let option = app.buttons[name]
        XCTAssertTrue(
            option.waitForExistence(timeout: 10),
            "the skill ring has no \(name)"
        )
        option.tap()
    }

    func testTheShellDrivesTheCore() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading"
        )

        let ready = waitForLabel(["walk", "stand"], timeout: 180)
        XCTAssertTrue(
            ["walk", "stand"].contains(ready),
            "the 50 Hz loop never drove the duck: label=\(ready)"
        )

        // A skill from the ring.
        try pickSkill("roulade")
        XCTAssertEqual(
            waitForLabel(["roulade"], timeout: 20), "roulade",
            "the roulade ring entry did not reach the core"
        )
        _ = waitForLabel(["walk", "stand"], timeout: 30)

        // The time machine: freeze, step, resume.
        let time = app.buttons["duck-time"]
        XCTAssertTrue(time.waitForExistence(timeout: 10), "the time button is missing")
        time.tap()
        XCTAssertTrue(
            app.staticTexts["This duck"].waitForExistence(timeout: 10),
            "the instrument sheet did not open"
        )
        let freeze = app.buttons["Freeze"]
        XCTAssertTrue(freeze.waitForExistence(timeout: 5), "the freeze button is missing")
        freeze.tap()
        XCTAssertTrue(
            app.buttons["Resume"].waitForExistence(timeout: 5),
            "freeze did not flip to resume"
        )
        app.buttons["Resume"].tap()

        // Close the sheet and keep driving.
        app.swipeDown()
        let after = waitForLabel(["walk", "stand"], timeout: 30)
        XCTAssertTrue(
            ["walk", "stand"].contains(after),
            "the duck stopped after the time machine: label=\(after)"
        )
    }
}

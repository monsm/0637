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
        // Portrait is the product; a simulator that was last used in
        // landscape would otherwise validate the wrong shell.
        XCUIDevice.shared.orientation = .portrait
        app = XCUIApplication()
        app.launch()
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline && app.frame.height <= app.frame.width {
            usleep(100_000)
        }
        XCTAssertTrue(
            app.frame.height > app.frame.width,
            "the app did not launch in portrait (\(app.frame))"
        )
    }

    override func tearDownWithError() throws {
        XCUIDevice.shared.orientation = .portrait
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
            // The live bar is the one the test watches; the sheet has its own.
            let visible = element.firstMatch
            if visible.exists {
                last = visible.label
                if wanted.contains(last) { return last }
            }
            usleep(100_000)
        }
        return last
    }

    /// Open the skill ring and pick a skill by its visible name.
    private func pickSkill(_ name: String) throws {
        let skills = app.buttons["duck-skills"]
        XCTAssertTrue(skills.waitForExistence(timeout: 10), "the skills button is missing")
        skills.tap()
        // The ring's entry carries a unique id: the same skill name can appear
        // in more than one place once the shell has a ring and a sheet.
        let option = app.buttons["ring-\(name)"]
        XCTAssertTrue(
            option.waitForExistence(timeout: 10),
            "the skill ring has no \(name)"
        )
        option.tap()
    }

    /// A menu item, found wherever SwiftUI put it. The popup's container
    /// element type varies by iOS version, so this searches every button
    /// descendant by identifier and falls back to the visible label.
    private func menuButton(_ id: String, _ titles: [String]) -> XCUIElement {
        let byId = app.descendants(matching: .button)[id]
        if byId.waitForExistence(timeout: 8) { return byId }
        let predicate = NSPredicate(
            format: titles.map { _ in "label BEGINSWITH %@" }.joined(separator: " OR "),
            argumentArray: titles
        )
        return app.descendants(matching: .button).matching(predicate).firstMatch
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

        // A skill from the ring. The moving label lasts only as long as the
        // move itself — under a second — and XCTest's own element query can
        // take longer than that on a busy simulator runner. Assert the durable
        // verdict the shell keeps from duck_core_trigger_skill's return value:
        // it proves the button reached the core and the core accepted it.
        try pickSkill("roulade")
        let verdict = app.staticTexts["duck-last-skill"]
        XCTAssertTrue(
            verdict.waitForExistence(timeout: 20),
            "the roulade ring entry did not reach the core"
        )
        XCTAssertEqual(verdict.label, "roulade", "the ring started the wrong skill")
        _ = waitForLabel(["walk", "stand"], timeout: 30)

        // The time machine: freeze, step, resume.
        let time = app.buttons["duck-time"]
        XCTAssertTrue(time.waitForExistence(timeout: 10), "the time button is missing")
        time.tap()
        // The sheet's own title, by identifier: the visible text changes with
        // the language, and the test must not depend on which one is saved.
        XCTAssertTrue(
            app.staticTexts["sheet-title"].waitForExistence(timeout: 10),
            "the instrument sheet did not open"
        )
        let freeze = app.buttons["time-pause.fill"]
        XCTAssertTrue(freeze.waitForExistence(timeout: 5), "the freeze button is missing")
        freeze.tap()
        XCTAssertTrue(
            app.buttons["time-play.fill"].waitForExistence(timeout: 5),
            "freeze did not flip to resume"
        )
        app.buttons["time-play.fill"].tap()

        // Close the sheet with its own button: in a portrait medium detent a
        // swipe scrolls the sheet's ScrollView instead of dismissing it.
        let sheetClose = app.buttons["sheet-close"]
        XCTAssertTrue(sheetClose.waitForExistence(timeout: 5), "the sheet has no close button")
        sheetClose.tap()
        let gone = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: app.staticTexts["sheet-title"]
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [gone], timeout: 5),
            .completed,
            "the sheet did not close"
        )
        let after = waitForLabel(["walk", "stand"], timeout: 30)
        XCTAssertTrue(
            ["walk", "stand"].contains(after),
            "the duck stopped after the time machine: label=\(after)"
        )

        // Director mode: the camera is unavailable in the simulator, and the
        // app must keep rendering the studio view and driving the duck rather
        // than prompt, block, or go black.
        // The camera is one tap on the top bar (the rest of the old chips
        // live behind the shell menu). Entering Director must not stop the
        // duck even though the simulator has no camera.
        let director = app.buttons["surface-toggle"]
        XCTAssertTrue(director.waitForExistence(timeout: 10), "no Director switch")
        director.tap()
        usleep(1_500_000)
        let inDirector = waitForLabel(["walk", "stand"], timeout: 20)
        XCTAssertTrue(
            ["walk", "stand"].contains(inDirector),
            "the duck stopped in Director mode: label=\(inDirector)"
        )
        app.buttons["surface-toggle"].tap()
        let backLive = waitForLabel(["walk", "stand"], timeout: 20)
        XCTAssertTrue(
            ["walk", "stand"].contains(backLive),
            "the duck stopped after leaving Director: label=\(backLive)"
        )
    }

    /// The same duck, the wide composition: rotation must swap shells without
    /// losing the core, the controls or the skill path.
    func testLandscapeRoundTrip() throws {
        XCTAssertTrue(labelElement().waitForExistence(timeout: 120), "the status panel never appeared")
        let ready = waitForLabel(["walk", "stand"], timeout: 180)
        XCTAssertTrue(["walk", "stand"].contains(ready), "the loop never drove: label=\(ready)")

        XCUIDevice.shared.orientation = .landscapeLeft
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline && app.frame.width <= app.frame.height {
            usleep(100_000)
        }
        XCTAssertTrue(
            app.frame.width > app.frame.height,
            "rotation to landscape did not happen (\(app.frame))"
        )
        // Every load-bearing control exists in the wide shell too.
        for id in ["duck-label", "duck-menu", "surface-toggle", "duck-time", "duck-skills", "duck-quack", "duck-grab"] {
            let element = app.buttons[id].exists ? app.buttons[id] : app.staticTexts[id]
            XCTAssertTrue(element.waitForExistence(timeout: 5), "\(id) missing in landscape")
        }
        try pickSkill("roulade")
        let verdict = app.staticTexts["duck-last-skill"]
        XCTAssertTrue(verdict.waitForExistence(timeout: 20), "the ring did not reach the core in landscape")
        XCTAssertEqual(verdict.label, "roulade")

        XCUIDevice.shared.orientation = .portrait
        let back = Date().addingTimeInterval(10)
        while Date() < back && app.frame.height <= app.frame.width {
            usleep(100_000)
        }
        let after = waitForLabel(["walk", "stand"], timeout: 30)
        XCTAssertTrue(["walk", "stand"].contains(after), "the duck stopped after rotating back: label=\(after)")
        XCTAssertTrue(app.buttons["duck-quack"].exists, "the portrait deck is missing after rotating back")
    }
}

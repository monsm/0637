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
        // The assertions below look for the English word the shell shows for
        // `walk`/`stand`. Pin the language so a Chinese simulator does not
        // turn every wait into a 180 s timeout.
        app.launchArguments += ["-duck.language", "EN"]
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
        // Any element type: SwiftUI may expose a labelled Text as a static
        // text or as a container element, and the test cares about the id.
        app.descendants(matching: .any)["duck-label"].firstMatch
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

    /// Any element carrying the identifier, whatever element TYPE SwiftUI
    /// gave it - plain-styled buttons have surfaced as "other" elements.
    private func anyElement(_ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }

    /// Open the More drawer: one tap from the stage (B4 §2).
    private func openMore() {
        let more = app.buttons["duck-more"]
        XCTAssertTrue(more.waitForExistence(timeout: 10), "the More button is missing")
        more.tap()
        XCTAssertTrue(
            app.staticTexts["more-title"].waitForExistence(timeout: 10),
            "the More drawer did not open"
        )
    }

    /// Close the drawer again; a function sheet that opened on top of it
    /// leaves the drawer underneath.
    private func closeMore() {
        let close = app.buttons["more-close"]
        if close.waitForExistence(timeout: 5) {
            var swipes = 0
            while !close.isHittable && swipes < 8 {
                app.swipeDown()
                swipes += 1
            }
            if close.isHittable { close.tap() }
        }
        // The drawer has to be gone before the next drawer opens, or the tap
        // lands on a sheet that is still animating away.
        let gone = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: app.staticTexts["more-title"]
        )
        _ = XCTWaiter.wait(for: [gone], timeout: 5)
    }

    /// Open a skill from the ring.
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

    /// Scroll a sheet until a control is actually tappable. The instrument
    /// sheet opens at its medium detent, where the lower cards live below the
    /// fold; XCUITest's existence check alone would pass on an unreachable
    /// element and the tap would fail.
    private func bringIntoView(
        _ element: XCUIElement,
        scroll: XCUIElement,
        maxSwipes: Int = 5
    ) {
        var swipes = 0
        while !(element.exists && element.isHittable) && swipes < maxSwipes {
            guard scroll.exists else { break }
            scroll.swipeUp()
            swipes += 1
        }
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

        // The companion card and the vet book's export are real controls on
        // the first screen, not hidden behind More.
        XCTAssertTrue(
            app.progressIndicators["care-hunger"].waitForExistence(timeout: 5),
            "the companion card has no hunger bar"
        )
        XCTAssertTrue(
            app.staticTexts["quests-title"].waitForExistence(timeout: 5),
            "the quests card is missing"
        )
        let sheetScroll = app.scrollViews.firstMatch
        let export = app.buttons["export-vetbook"]
        bringIntoView(export, scroll: sheetScroll)
        XCTAssertTrue(export.waitForExistence(timeout: 5), "the vet book export is missing")
        XCTAssertTrue(export.isHittable, "the vet book export is not tappable")

        // The user's own model: the editor is reachable and its controls exist.
        let aiOpen = app.buttons["ai-open"]
        bringIntoView(aiOpen, scroll: sheetScroll)
        XCTAssertTrue(aiOpen.waitForExistence(timeout: 5), "no AI companion entry")
        aiOpen.tap()
        XCTAssertTrue(
            app.staticTexts["ai-title"].waitForExistence(timeout: 10),
            "the AI companion sheet did not open"
        )
        XCTAssertTrue(
            app.textFields["ai-endpoint"].waitForExistence(timeout: 5),
            "the AI sheet has no endpoint field"
        )
        app.buttons["ai-close"].tap()
        XCTAssertTrue(
            app.staticTexts["sheet-title"].waitForExistence(timeout: 5),
            "closing the AI sheet did not return to the instrument"
        )
        // The header scrolled away with the rest of the sheet content; bring
        // its close target back before using it.
        let sheetClose = app.buttons["sheet-close"]
        var scrollBack = 0
        while !sheetClose.isHittable && scrollBack < 5 {
            sheetScroll.swipeDown()
            scrollBack += 1
        }

        // Close the sheet with its own button: in a portrait medium detent a
        // swipe scrolls the sheet's ScrollView instead of dismissing it.
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
        // The block program: add "when clap, roll over" through the editor and
        // see the block it created. The engine itself is exercised by
        // sim-drive's clap test; this proves the control is wired to it. It
        // now lives in the More drawer (B4 §2), one tap from the stage.
        openMore()
        let rulesButton = app.buttons["more-blocks"]
        XCTAssertTrue(rulesButton.waitForExistence(timeout: 10), "the drawer has no Blocks row")
        rulesButton.tap()
        XCTAssertTrue(
            app.staticTexts["rules-title"].waitForExistence(timeout: 10),
            "the block editor did not open"
        )
        let addRule = app.buttons["rule-add"]
        XCTAssertTrue(addRule.waitForExistence(timeout: 5), "the Add block button is missing")
        addRule.tap()
        XCTAssertTrue(
            app.otherElements["rule-0"].waitForExistence(timeout: 5)
                || app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "roulade")).firstMatch.waitForExistence(timeout: 5),
            "the rule was not added to the list"
        )
        app.buttons["rules-close"].tap()
        let rulesGone = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: app.staticTexts["rules-title"]
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [rulesGone], timeout: 5),
            .completed,
            "the block editor did not close"
        )
        closeMore()
        // The neighbours sheet: with no second phone in CI it must say so
        // rather than sit blank, and its close target must work.
        openMore()
        let peersButton = app.buttons["more-peers"]
        XCTAssertTrue(peersButton.waitForExistence(timeout: 10), "the drawer has no Neighbours row")
        peersButton.tap()
        XCTAssertTrue(
            app.staticTexts["peers-title"].waitForExistence(timeout: 10),
            "the neighbours sheet did not open"
        )
        XCTAssertTrue(
            app.staticTexts["peers-empty"].waitForExistence(timeout: 5),
            "an empty LAN has no empty-state line"
        )
        app.buttons["peers-close"].tap()
        closeMore()
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
        // The shutter is real: it reads the frame back, writes a PNG and opens
        // the preview, even with no camera (the studio is the room then).
        let shutter = app.buttons["duck-photo"]
        XCTAssertTrue(shutter.waitForExistence(timeout: 10), "no Director shutter")
        shutter.tap()
        let preview = app.descendants(matching: .any)
            .matching(identifier: "photo-preview")
            .firstMatch
        XCTAssertTrue(preview.waitForExistence(timeout: 15), "the Director photo never reached a preview")
        // The caption is read from /state asynchronously; wait for the real
        // line (never empty) before keeping, or the postcard band is bare.
        XCTAssertTrue(
            app.staticTexts["postcard-caption"].waitForExistence(timeout: 10),
            "the postcard preview has no caption yet"
        )
        // Keep writes the composed postcard (frame + caption band + date)
        // into Documents/Postcards; CI publishes the newest one as the
        // acceptance artifact. The button must really flip to "kept".
        let keep = app.buttons["photo-keep"]
        XCTAssertTrue(keep.waitForExistence(timeout: 5), "the photo preview has no keep button")
        keep.tap()
        let kept = XCTNSPredicateExpectation(
            predicate: NSPredicate(
                format: "label CONTAINS %@ OR label CONTAINS %@",
                "Kept", "已收藏"
            ),
            object: keep
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [kept], timeout: 10),
            .completed,
            "keeping the photo did not write a postcard"
        )
        let photoClose = app.buttons["photo-close"]
        XCTAssertTrue(photoClose.waitForExistence(timeout: 5), "the photo preview has no close button")
        photoClose.tap()
        XCTAssertTrue(
            shutter.waitForExistence(timeout: 10),
            "the shutter did not return after the photo preview closed"
        )
        app.buttons["surface-toggle"].tap()
        let backLive = waitForLabel(["walk", "stand"], timeout: 20)
        XCTAssertTrue(
            ["walk", "stand"].contains(backLive),
            "the duck stopped after leaving Director: label=\(backLive)"
        )
    }

    /// The DKPD sender's UI: with a fixture peer, the paired-peer gift rows
    /// and the pad-send chip must actually be on the sheet — the chip used
    /// to be built inside the gift-emotion action closure and never entered
    /// the tree (FE-1). Tapping gift-emotion must survive (the core refuses
    /// a gift to a fixture id, which is honest), and tapping pad-send must
    /// flip the chip into its driving label.
    func testPairedPeerRowsRenderAndRespond() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading"
        )
        app.terminate()
        app.launchArguments += ["-duck.peerFixture", "YES"]
        app.launch()
        openMore()
        let peersButton = app.buttons["more-peers"]
        XCTAssertTrue(peersButton.waitForExistence(timeout: 10), "the drawer has no Neighbours row")
        // Bisect at the drawer: its badge reads the same controller.peers
        // the sheet will read, so this splits "fixture never reached the
        // controller" from "the sheet failed to render it".
        let badge = app.staticTexts["1/3"]
        XCTAssertTrue(
            badge.waitForExistence(timeout: 5)
                || peersButton.label.contains("1/3"),
            "the fixture peer never reached the controller (badge='\(peersButton.label)')"
        )
        peersButton.tap()
        XCTAssertTrue(
            app.staticTexts["peers-title"].waitForExistence(timeout: 10),
            "the neighbours sheet did not open"
        )
        XCTAssertFalse(
            app.staticTexts["peers-empty"].exists,
            "the sheet still shows the empty state with a fixture peer"
        )
        // The chips live inside a sheet-presented ForEach row; XCUITest's
        // element TYPE for a plain-styled button has drifted before, so
        // query any element carrying the identifier rather than betting on
        // one type - the tree itself was already proven above.
        let treat = anyElement("gift-treat-fixture")
        if !treat.exists {
            // The bisect proved the row composes; dump the identifiers the
            // tree actually carries, so the next fix is not another guess.
            let lines = app.debugDescription.split(separator: "\n")
            let hits = lines.enumerated().compactMap { offset, line -> String? in
                let text = String(line)
                return text.contains("peer-row") || text.contains("gift")
                    || text.contains("pad-send") || text.contains("Sunny")
                    ? "\(offset): \(text.prefix(160))"
                    : nil
            }
            print("TREE-DIAG lines=\(lines.count) hits=\(hits.prefix(16))")
            let anyIds = lines.filter { $0.contains("identifier") }.prefix(20)
            print("TREE-DIAG first-identifiers=\(anyIds)")
        }
        XCTAssertTrue(treat.waitForExistence(timeout: 10), "the fixture peer's gift-treat row is missing")
        XCTAssertTrue(anyElement("gift-emotion-fixture").exists, "the gift-emotion chip is missing")
        let padSend = anyElement("pad-send-fixture")
        XCTAssertTrue(padSend.waitForExistence(timeout: 10), "the pad-send chip never rendered (FE-1)")
        treat.tap()
        XCTAssertTrue(
            app.staticTexts["peers-title"].waitForExistence(timeout: 5),
            "tapping a gift took the sheet down"
        )
        padSend.tap()
        XCTAssertEqual(
            padSend.label, "Driving",
            "tapping pad-send did not start the pad"
        )
        app.buttons["peers-close"].tap()
        closeMore()
    }

    /// The day rows the core publishes (per-day metres/powered/rolls) must
    /// reach the duck sheet — the data loaded since batch six but no view
    /// consumed it (FE-3). The fixture pins two synthetic rows; the view
    /// shows them verbatim and never invents numbers.
    func testDayRowsShowInTheDuckSheet() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading"
        )
        app.terminate()
        app.launchArguments += ["-duck.dayFixture", "YES"]
        app.launch()
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared on the relaunch"
        )
        let timeButton = app.buttons["duck-time"]
        XCTAssertTrue(timeButton.waitForExistence(timeout: 10), "the time rail has no Time button")
        timeButton.tap()
        XCTAssertTrue(
            app.staticTexts["sheet-title"].waitForExistence(timeout: 10),
            "the duck sheet did not open"
        )
        let rows = app.staticTexts["day-rows"]
        // The life card sits below the medium detent's fold; existence alone
        // would pass on an unreachable element, so scroll it into view.
        bringIntoView(rows, scroll: app.scrollViews.firstMatch)
        XCTAssertTrue(rows.waitForExistence(timeout: 10), "the day rows never reached the duck sheet (FE-3)")
        XCTAssertTrue(
            rows.label.contains("2026-11-15"),
            "the day rows lost the newest day: \(rows.label)"
        )
        app.buttons["sheet-close"].tap()
    }

    /// The Path Painter: the mode opens from the More drawer, and with
    /// nothing drawn Go must exist but wait for a route with two real steps.
    func testPathModeOpensWithGoWaiting() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading"
        )
        let ready = waitForLabel(["walk", "stand"], timeout: 180)
        XCTAssertTrue(
            ["walk", "stand"].contains(ready),
            "the 50 Hz loop never drove the duck: label=\(ready)"
        )

        // One tap into the stage-as-canvas: the drawer's Path row (B4 §2).
        openMore()
        let pathRow = app.buttons["more-path"]
        XCTAssertTrue(pathRow.waitForExistence(timeout: 10), "the drawer has no Path row")
        pathRow.tap()

        let go = app.buttons["path-go"]
        XCTAssertTrue(go.waitForExistence(timeout: 10), "path mode has no Go chip")
        XCTAssertFalse(go.isEnabled, "Go is enabled with nothing drawn")
        XCTAssertTrue(
            app.buttons["path-clear"].waitForExistence(timeout: 5),
            "path mode has no Clear chip"
        )
        // Nothing is walking, so no step chip may be on the stage either.
        XCTAssertFalse(
            app.staticTexts["path-progress"].exists,
            "a step chip appeared with no route"
        )
    }

    /// The B1 rule, asserted where it can regress: the live first screen
    /// speaks human words. No protocol state token, no `t <number>` tick, no
    /// percentage in the mood line, and a `kick_left` verdict without `_`.
    func testFirstScreenSpeaksHuman() throws {
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the status panel never appeared - the app did not get past loading\n\(app.debugDescription)"
        )
        let ready = waitForLabel(["walk", "stand"], timeout: 180)
        XCTAssertTrue(
            ["walk", "stand"].contains(ready),
            "the 50 Hz loop never drove the duck: label=\(ready)"
        )

        // The tick belongs to the time machine, not the first screen.
        let tickTexts = app.staticTexts.matching(
            NSPredicate(format: "label MATCHES %@", "t [0-9]+")
        ).count
        XCTAssertEqual(tickTexts, 0, "a raw tick is on the first screen")

        // The mood capsule says how it feels, never a percentage.
        let mood = app.staticTexts["mood-pill"]
        XCTAssertTrue(mood.waitForExistence(timeout: 20), "the mood capsule never appeared")
        XCTAssertFalse(mood.label.contains("%"), "the mood line still carries a percentage: \(mood.label)")

        // The B4 budget: seven tappable controls on the first screen —
        // four verbs, More, and the two time-rail buttons.
        let tappable = app.buttons.allElementsBoundByIndex.filter { $0.exists && $0.isHittable }
        XCTAssertLessThanOrEqual(
            tappable.count, 7,
            "the first screen has \(tappable.count) tappable controls"
        )

        // The daily goals live in the More drawer now, one tap from the stage.
        openMore()
        XCTAssertTrue(
            app.buttons["more-quests"].waitForExistence(timeout: 5),
            "the drawer has no Daily quests row"
        )
        XCTAssertTrue(
            app.buttons["more-diary"].exists,
            "the drawer has no Diary row"
        )
        XCTAssertTrue(
            app.buttons["more-postcards"].exists,
            "the drawer has no Postcards row"
        )
        XCTAssertTrue(
            app.buttons["more-dex"].exists,
            "the drawer has no Mood book row"
        )
        closeMore()

        // A wire skill name must never reach the stage. `kick_left` is the
        // case with an underscore the old bar printed verbatim.
        try pickSkill("kick_left")
        let verdict = app.staticTexts["duck-last-skill"]
        XCTAssertTrue(verdict.waitForExistence(timeout: 20), "the kick never reached the core")
        XCTAssertFalse(
            verdict.label.contains("_"),
            "the first screen shows a raw skill name: \(verdict.label)"
        )
        XCTAssertFalse(
            labelElement().label.contains("_"),
            "the first screen shows a raw state token: \(labelElement().label)"
        )
    }

    /// A composed postcard in Chinese, for B4 §6.5. It sorts last (Z) so the
    /// newest card in the app container is this one; CI publishes it as the
    /// Chinese sample.
    func testZChinesePostcard() throws {
        app.terminate()
        app.launchArguments = ["-duck.language", "中文"]
        app.launch()
        XCTAssertTrue(
            labelElement().waitForExistence(timeout: 120),
            "the app did not get past loading"
        )
        let director = app.buttons["surface-toggle"]
        XCTAssertTrue(director.waitForExistence(timeout: 10), "no Director switch")
        director.tap()
        usleep(1_500_000)
        let shutter = app.buttons["duck-photo"]
        XCTAssertTrue(shutter.waitForExistence(timeout: 10), "no Director shutter")
        shutter.tap()
        XCTAssertTrue(
            app.staticTexts["postcard-caption"].waitForExistence(timeout: 15),
            "the Chinese postcard has no caption"
        )
        let keep = app.buttons["photo-keep"]
        XCTAssertTrue(keep.waitForExistence(timeout: 5), "no keep button")
        keep.tap()
        let kept = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "label CONTAINS %@", "已收藏"),
            object: keep
        )
        XCTAssertEqual(
            XCTWaiter.wait(for: [kept], timeout: 10),
            .completed,
            "the Chinese postcard was not kept"
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
        // The wide shell still carries its own ⋯ menu this round (B4a is
        // portrait only), so the landscape keeps that entry.
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

    // MARK: - Voice

    /// The duck's voice is part of the product, not a UI decoration: the one
    /// audio session must be a category that reaches the speaker, and the
    /// quack must really reach AVAudioEngine. This is the regression the
    /// simulator otherwise hides: a play-and-record session without
    /// `.defaultToSpeaker` routes to the receiver on a phone, while the
    /// simulator keeps the speaker and CI stays green.
    func testTheDuckVoiceIsHeard() throws {
        // The product path: no microphone, so the session must be playback
        // (not ambient: the Ring/Silent switch would silence that).
        launchForVoice([
            "-mic_enabled", "NO",
            "-voice_commands_enabled", "NO",
        ])
        let ambient = waitForVoiceLine(timeout: 180)
        XCTAssertTrue(
            ambient.contains("cat=AVAudioSessionCategoryPlayback"),
            "playback-only audio is not a speaker-capable category: \(ambient)"
        )
        XCTAssertTrue(ambient.contains("mic=0"), "the microphone tap is unexpectedly live: \(ambient)")
        XCTAssertTrue(ambient.contains("active=1"), "the audio session is not active: \(ambient)")
        XCTAssertTrue(ambient.contains("engine=1"), "the audio engine is not running: \(ambient)")
        XCTAssertTrue(
            ambient.contains("recOpts=mixWithOthers,defaultToSpeaker"),
            "the record path lost .defaultToSpeaker (receiver routing): \(ambient)"
        )
        try assertQuackReachesTheEngine()

        // The regression path: what an enabled room mic or speech ear
        // creates at launch. CI cannot grant microphone permission, so the
        // DEBUG flag takes the same option list without an input tap.
        launchForVoice([
            "-mic_enabled", "NO",
            "-voice_commands_enabled", "NO",
            "-duck.voiceForceRecordPath", "YES",
        ])
        let record = waitForVoiceLine(timeout: 180)
        XCTAssertTrue(
            record.contains("cat=AVAudioSessionCategoryPlayAndRecord"),
            "the forced record path did not switch category: \(record)"
        )
        XCTAssertTrue(record.contains("forced=1"), "the forced record flag did not reach the voice: \(record)")
        XCTAssertTrue(
            record.contains("opts=mixWithOthers,defaultToSpeaker"),
            "play-and-record lost .defaultToSpeaker, so iOS routes to the receiver: \(record)"
        )
        XCTAssertTrue(record.contains("engine=1"), "the engine is not running in record mode: \(record)")
        XCTAssertTrue(record.contains("active=1"), "the session is not active in record mode: \(record)")
        try assertQuackReachesTheEngine()
    }

    /// The diary's speak channel: the system voice reading the real facts
    /// aloud, honestly labelled as the phone speaking FOR the duck (not the
    /// duck's own synth). A fresh install has no facts yet, so the button may
    /// not exist — the note always does, and a tap that happens must leave
    /// the sheet alive. The synthesizer itself is device-side; CI only proves
    /// the wiring.
    func testDiarySpeakChannelIsHonest() throws {
        openMore()
        let diaryRow = app.buttons["more-diary"]
        XCTAssertTrue(diaryRow.exists, "the drawer has no Diary row")
        diaryRow.tap()
        XCTAssertTrue(
            app.staticTexts["diary-title"].waitForExistence(timeout: 10),
            "the diary sheet did not open"
        )
        XCTAssertTrue(
            app.staticTexts["diary-speak-note"].waitForExistence(timeout: 10),
            "the diary never says the system voice speaks for the duck"
        )
        let speak = app.buttons["diary-speak"]
        if speak.exists {
            // A device with no voices finishes instantly, so whether the
            // button flipped is device-side and racy — the wiring assertion
            // is that a tap leaves the sheet alive and the note still honest.
            speak.tap()
            usleep(1_000_000)
            XCTAssertTrue(
                app.staticTexts["diary-title"].exists,
                "speaking took the diary sheet down"
            )
            XCTAssertTrue(
                app.staticTexts["diary-speak-note"].exists,
                "speaking took the honest label away"
            )
        }
        let close = app.buttons["diary-close"]
        XCTAssertTrue(close.waitForExistence(timeout: 5), "the diary has no close button")
        close.tap()
        closeMore()
    }

    /// B7: one route table, and a destination returns where it came from.
    /// The drawer's rows nest over the drawer; the instrument's secondary
    /// entries nest over the instrument. This pins the adapter behaviour the
    /// meeting said must not be deleted as "zombies" - and it is what makes
    /// "close the AI sheet and you are back in the instrument" true.
    func testRouteTableReturnsToThePresentingSheet() throws {
        openMore()
        let collectionRow = app.buttons["more-dex"]
        XCTAssertTrue(
            collectionRow.waitForExistence(timeout: 5),
            "the drawer has no collection row"
        )
        collectionRow.tap()
        XCTAssertTrue(
            app.staticTexts["collection-title"].waitForExistence(timeout: 10),
            "the collection sheet did not open from the drawer"
        )
        app.buttons["collection-close"].tap()
        XCTAssertTrue(
            app.staticTexts["more-title"].waitForExistence(timeout: 10),
            "closing the collection did not return to the drawer"
        )
        closeMore()

        let time = app.buttons["duck-time"]
        XCTAssertTrue(time.waitForExistence(timeout: 10), "the time button is missing")
        time.tap()
        XCTAssertTrue(
            app.staticTexts["sheet-title"].waitForExistence(timeout: 10),
            "the instrument did not open"
        )
        let collectionOpen = app.buttons["collection-open"]
        bringIntoView(collectionOpen, scroll: app.scrollViews.firstMatch)
        XCTAssertTrue(collectionOpen.exists, "the instrument has no collection entry")
        collectionOpen.tap()
        XCTAssertTrue(
            app.staticTexts["collection-title"].waitForExistence(timeout: 10),
            "the collection sheet did not open from the instrument"
        )
        app.buttons["collection-close"].tap()
        XCTAssertTrue(
            app.staticTexts["sheet-title"].waitForExistence(timeout: 10),
            "closing the collection did not return to the instrument"
        )
        app.buttons["sheet-close"].tap()
    }

    /// Relaunch with the voice diagnostics open and the given argument-domain
    /// overrides. The overrides keep a persisted mic toggle from an earlier
    /// test out of this one.
    private func launchForVoice(_ extra: [String]) {
        app.terminate()
        app.launchArguments = ["-duck.language", "EN", "-duck.voiceDiagnostics", "YES"] + extra
        app.launch()
    }

    private func voiceStateElement() -> XCUIElement {
        // Any element type: SwiftUI may expose the labelled Text as a static
        // text or as a container, and the test cares about the id.
        app.descendants(matching: .any)["duck-voice-state"].firstMatch
    }

    /// The diagnostic line arrives only after the core is up (seconds on a
    /// loaded CI simulator); poll rather than sleeping once.
    private func waitForVoiceLine(timeout: TimeInterval) -> String {
        let element = voiceStateElement()
        let deadline = Date().addingTimeInterval(timeout)
        var line = ""
        while Date() < deadline {
            if element.exists {
                line = element.label
                if line.contains("ready=1") { return line }
            }
            usleep(200_000)
        }
        return line
    }

    private func voiceField(_ key: String, in line: String) -> String {
        for part in line.split(separator: " ") {
            if part.hasPrefix(key + "=") {
                return String(part.dropFirst(key.count + 1))
            }
        }
        return ""
    }

    /// Tap the quack button and prove the PCM reached `scheduleBuffer`: the
    /// core queue alone is not a spoken duck.
    private func assertQuackReachesTheEngine() throws {
        let quack = app.buttons["duck-quack"]
        XCTAssertTrue(quack.waitForExistence(timeout: 30), "the quack button is missing")
        var line = waitForVoiceLine(timeout: 30)
        let before = Int(voiceField("sched", in: line)) ?? 0
        quack.tap()
        // The core keeps one one-shot sound and a later idle emotion replaces
        // a queued quack — that is the "one voice" rule, and tapping again is
        // the interaction it allows. Keep asking until one reaches the engine.
        var lastTap = Date()
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            line = voiceStateElement().exists ? voiceStateElement().label : line
            let now = Int(voiceField("sched", in: line)) ?? 0
            if now > before && voiceField("tag", in: line) == "chirp" { break }
            if Date().timeIntervalSince(lastTap) >= 1 {
                quack.tap()
                lastTap = Date()
            }
            usleep(200_000)
        }
        let after = Int(voiceField("sched", in: line)) ?? 0
        XCTAssertGreaterThan(after, before, "the quack never reached the audio engine: \(line)")
        XCTAssertEqual(
            voiceField("tag", in: line), "chirp",
            "the scheduled sound was not the quack: \(line)"
        )
        XCTAssertEqual(
            voiceField("engine", in: line), "1",
            "the engine stopped while quacking: \(line)"
        )
        XCTAssertEqual(
            voiceField("player", in: line), "1",
            "the player node is not playing after the quack: \(line)"
        )
    }

    /// D1: the first screen at the largest accessibility text size. The deck
    /// labels grow (two lines allowed), their containers grow with them, and
    /// nothing leaves the screen or overlaps a neighbour.
    func testFirstScreenSurvivesAccessibilityXXXL() throws {
        app.terminate()
        app = XCUIApplication()
        app.launchArguments += [
            "-duck.language", "EN",
            "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL",
        ]
        app.launch()
        let ids = ["duck-feed", "duck-skills", "duck-quack", "duck-grab", "duck-more"]
        var frames: [CGRect] = []
        for id in ids {
            let button = app.buttons[id]
            XCTAssertTrue(button.waitForExistence(timeout: 30), "\(id) is missing at Accessibility XXXL")
            XCTAssertTrue(button.isHittable, "\(id) is not tappable at Accessibility XXXL")
            frames.append(button.frame)
        }
        let screen = app.frame.insetBy(dx: -1, dy: -1)
        for (index, frame) in frames.enumerated() {
            XCTAssertTrue(screen.contains(frame), "\(ids[index]) leaves the screen: \(frame) in \(app.frame)")
        }
        for i in frames.indices {
            for j in frames.indices where j > i {
                XCTAssertFalse(frames[i].intersects(frames[j]), "\(ids[i]) overlaps \(ids[j])")
            }
        }
        XCTAssertTrue(
            app.buttons["duck-time"].waitForExistence(timeout: 10),
            "the time rail vanished at Accessibility XXXL"
        )
        openMore()
        XCTAssertTrue(
            app.staticTexts["more-title"].exists,
            "the drawer title vanished at Accessibility XXXL"
        )
        XCTAssertTrue(
            app.buttons["more-close"].isHittable,
            "the drawer close is unreachable at Accessibility XXXL"
        )
    }

    /// D4 sweep: every dense sheet at Accessibility XXXL. Each one opens
    /// with its title as the first focus stop, its close and key controls
    /// stay tappable at 44 pt, and closing the drawer returns focus to the
    /// button that opened it (the probe value is the real
    /// @AccessibilityFocusState the modifier drives).
    func testDenseSheetsSurviveAccessibilityXXXL() throws {
        app.terminate()
        app = XCUIApplication()
        app.launchArguments += [
            "-duck.language", "EN",
            "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL",
            "-duck.focusProbe", "YES",
        ]
        app.launch()
        let more = app.buttons["duck-more"]
        XCTAssertTrue(more.waitForExistence(timeout: 30), "the More button is missing at Accessibility XXXL")
        let moreProbes = focusProbeCount()
        more.tap()

        let moreControls = [
            "more-close", "more-path", "more-blocks", "more-dance", "more-playmore",
            "more-peers", "more-chorus", "more-gifts", "more-p2", "more-quests",
            "more-diary", "more-postcards", "more-dex", "more-ai", "more-night",
            "more-duckpanel", "more-tools",
        ]
        assertSheetFocus("more-title", sheet: "More", probesBefore: moreProbes)
        assertHitTargets(sheet: "More", ids: moreControls)

        openDense("more-peers", title: "peers-title", close: "peers-close", sheet: "Neighbours",
                  controls: ["peers-close", "ensemble-formation", "ensemble-roles",
                             "ensemble-choir", "chorus-start", "peers-empty-action"])
        openDense("more-blocks", title: "rules-title", close: "rules-close", sheet: "Blocks",
                  controls: ["rules-close", "rules-share", "rules-paste"])
        openDense("more-dance", title: "dance-title", close: "dance-close", sheet: "Dance",
                  controls: ["dance-close", "dance-record", "dance-play", "dance-clear",
                             "dance-try-roulade"])
        openDense("more-ai", title: "ai-title", close: "ai-close", sheet: "AI",
                  controls: ["ai-close", "ai-save", "ai-think", "chat-send"])
        openDense("more-diary", title: "diary-title", close: "diary-close", sheet: "Diary",
                  controls: ["diary-close", "diary-empty-state-action"])
        openDense("more-postcards", title: "postcards-title", close: "postcards-close", sheet: "Postcards",
                  controls: ["postcards-close", "postcards-empty-state-action"])
        openDense("more-dex", title: "collection-title", close: "collection-close", sheet: "Collection",
                  controls: ["collection-close", "collection-refresh"])
        openDense("more-quests", title: "progress-title", close: "progress-close", sheet: "Progress",
                  controls: ["progress-close"])
        openDense("more-playmore", title: "play-more-title", close: "sheet-close-shared", sheet: "Play more",
                  controls: ["sheet-close-shared", "play-more-mode-drive",
                             "play-more-ab-load", "play-more-push", "play-more-hud"])
        openDense("more-duckpanel", title: "duck-panel-title", close: "sheet-close-shared", sheet: "Duck panel",
                  controls: ["sheet-close-shared", "duck-panel-heart", "duck-panel-flashlight",
                             "duck-panel-language", "duck-panel-compass", "alias-open"])
        openDense("more-tools", title: "tools-title", close: "sheet-close-shared", sheet: "Tools",
                  controls: ["sheet-close-shared", "tools-parent", "tools-lamp",
                             "tools-backup-export", "tools-backup-import", "tools-tof",
                             "duck-panel-big", "days-open"])

        // Nested dense pages: words under the duck panel, days and big
        // buttons under tools.
        app.buttons["more-duckpanel"].tap()
        XCTAssertTrue(anyElement("duck-panel-title").waitForExistence(timeout: 10))
        let aliasProbes = focusProbeCount()
        XCTAssertTrue(tapWhenVisible(app.buttons["alias-open"]), "Words: alias row unreachable")
        assertSheetFocus("alias-title", sheet: "Words", probesBefore: aliasProbes)
        assertHitTargets(sheet: "Words", ids: ["alias-close", "alias-add"])
        app.buttons["alias-close"].tap()
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: anyElement("alias-title")
        )], timeout: 6)
        app.buttons["sheet-close-shared"].tap()

        app.buttons["more-tools"].tap()
        XCTAssertTrue(anyElement("tools-title").waitForExistence(timeout: 10))
        let daysProbes = focusProbeCount()
        XCTAssertTrue(tapWhenVisible(app.buttons["days-open"]), "Days: days row unreachable")
        assertSheetFocus("days-title", sheet: "Days", probesBefore: daysProbes)
        assertHitTargets(sheet: "Days", ids: ["days-close", "days-empty-state-action"])
        app.buttons["days-close"].tap()
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: anyElement("days-title")
        )], timeout: 6)
        let bigProbes = focusProbeCount()
        XCTAssertTrue(tapWhenVisible(app.buttons["duck-panel-big"]), "Big buttons: row unreachable")
        assertSheetFocus("big-title", sheet: "Big buttons", probesBefore: bigProbes)
        assertHitTargets(sheet: "Big buttons",
                         ids: ["big-close", "big-action-0", "big-action-1", "big-action-2", "big-action-3"])
        app.buttons["big-close"].tap()
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: anyElement("big-title")
        )], timeout: 6)
        app.buttons["sheet-close-shared"].tap()

        // The drawer closes and focus returns to the More button.
        closeMore()
        assertStageFocus("duck-more", focus: "more", sheet: "More return")

        // The instrument is the densest surface: same focus and hit rules.
        let instrumentProbes = focusProbeCount()
        app.buttons["duck-time"].tap()
        assertSheetFocus("sheet-title", sheet: "Instrument", probesBefore: instrumentProbes)
        assertHitTargets(sheet: "Instrument", ids: [
            "sheet-close", "time-pause.fill", "export-vetbook", "vetbook-card-open",
            "collection-open", "postcards-open", "diary-open", "ai-open",
        ])
        app.buttons["sheet-close"].tap()
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: anyElement("sheet-title")
        )], timeout: 6)
        assertStageFocus("duck-time", focus: "time", sheet: "Instrument return")
    }

    /// XCUITest cannot inspect VoiceOver focus directly; each sheet title
    /// carries a test-only probe the first-focus modifier arms on appear, so
    /// a new probe past the pre-open count proves this sheet's first stop
    /// ran and set @AccessibilityFocusState.
    private func focusProbeCount() -> Int {
        app.staticTexts.matching(identifier: "sheet-focus-probe").count
    }

    private func assertSheetFocus(_ id: String, sheet: String, probesBefore: Int) {
        let element = anyElement(id)
        XCTAssertTrue(element.waitForExistence(timeout: 10), "\(sheet): the first element \(id) is missing")
        let deadline = Date().addingTimeInterval(6)
        while Date() < deadline {
            if focusProbeCount() > probesBefore { return }
            usleep(100_000)
        }
        XCTFail("\(sheet): the first-focus anchor never armed for \(id)")
    }

    /// Closing returns focus to the stage control that opened the sheet; its
    /// test-only badge is armed only while no sheet is up.
    private func assertStageFocus(_ id: String, focus: String, sheet: String) {
        let element = app.buttons[id]
        XCTAssertTrue(element.waitForExistence(timeout: 10), "\(sheet): \(id) is missing after close")
        XCTAssertTrue(
            app.staticTexts["stage-focus-\(focus)"].waitForExistence(timeout: 6),
            "\(sheet): \(id) did not regain focus"
        )
    }

    /// Tap an element that may have scrolled below the fold at AX3. Swipes
    /// the topmost scroll view until the element is actually hittable; a
    /// plain tap on an off-screen row fails without a useful message.
    private func tapWhenVisible(_ element: XCUIElement, swipes: Int = 8) -> Bool {
        var count = 0
        while !(element.exists && element.isHittable) && count < swipes {
            app.swipeUp()
            count += 1
        }
        guard element.exists && element.isHittable else { return false }
        element.tap()
        return true
    }

    /// Open a drawer row's sheet, assert its first-focus element and the
    /// 44 pt floor of the named controls, then close it back to the drawer.
    private func openDense(
        _ row: String,
        title: String,
        close: String,
        sheet: String,
        controls: [String]
    ) {
        let button = app.buttons[row]
        XCTAssertTrue(button.waitForExistence(timeout: 10), "\(sheet): the drawer row \(row) is missing")
        let probesBefore = focusProbeCount()
        XCTAssertTrue(tapWhenVisible(button), "\(sheet): the drawer row \(row) never became reachable")
        assertSheetFocus(title, sheet: sheet, probesBefore: probesBefore)
        assertHitTargets(sheet: sheet, ids: controls)
        let closeButton = app.buttons[close]
        XCTAssertTrue(closeButton.waitForExistence(timeout: 5), "\(sheet): close \(close) is missing")
        XCTAssertTrue(closeButton.isHittable, "\(sheet): close \(close) is unreachable")
        closeButton.tap()
        _ = XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: anyElement(title)
        )], timeout: 6)
        XCTAssertTrue(
            app.staticTexts["more-title"].waitForExistence(timeout: 6),
            "\(sheet): closing did not return to the drawer"
        )
    }

    /// The 44 pt audit for the controls the sheet really shows: each named
    /// element, when hittable, must have a full-size target.
    private func assertHitTargets(sheet: String, ids: [String]) {
        for id in ids {
            let element = anyElement(id)
            guard element.exists && element.isHittable else { continue }
            let frame = element.frame
            XCTAssertGreaterThanOrEqual(
                frame.height, 43,
                "\(sheet): \(id) is \(frame.height) pt tall at Accessibility XXXL"
            )
            XCTAssertGreaterThanOrEqual(
                frame.width, 43,
                "\(sheet): \(id) is \(frame.width) pt wide at Accessibility XXXL"
            )
        }
    }

}

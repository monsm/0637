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
        if close.waitForExistence(timeout: 5) { close.tap() }
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
            "the drawer has no Mood dex row"
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

}

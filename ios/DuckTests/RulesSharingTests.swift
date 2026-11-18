import XCTest
@testable import Duck

/// The block program's wire shape (comparison 2026-11-18 #24): a shared
/// program and the phone's saved copy must keep a timer's `period_s`, and
/// the paste vocabulary must be the core's own trigger list
/// (`core/duck-mobile-core/src/rules.rs:16`). These are the pure halves of
/// share/save/import; the core round trip is the simulator's job.
final class RulesSharingTests: XCTestCase {

    private func timer(period: Int?) -> DuckController.DuckRule {
        DuckController.DuckRule(
            trigger: "timer",
            action: "skill",
            value: "roulade",
            periodS: period
        )
    }

    private func document(_ rules: String) -> String {
        #"{"microduck-rules":1,"rules":\#(rules)}"#
    }

    func testSavedProgramRoundTripKeepsPeriod() throws {
        let rule = DuckController.DuckRule(
            trigger: "timer",
            action: "sound",
            value: "chirp",
            periodS: 60
        )
        let data = try JSONEncoder().encode([rule])
        let decoded = try JSONDecoder().decode(
            [DuckController.DuckRule].self,
            from: data
        )
        XCTAssertEqual(
            decoded,
            [rule],
            "the UserDefaults copy must keep a timer's period"
        )
    }

    func testProgramSavedBeforeTimersStillDecodes() throws {
        let text = #"[{"trigger":"clap","action":"skill","value":"roulade"}]"#
        let decoded = try JSONDecoder().decode(
            [DuckController.DuckRule].self,
            from: Data(text.utf8)
        )
        XCTAssertEqual(decoded.count, 1)
        XCTAssertEqual(decoded.first?.trigger, "clap")
        XCTAssertNil(decoded.first?.periodS)
    }

    func testWireObjectCarriesPeriodOnlyForTimer() {
        let payload = DuckController.ruleWireObject(timer(period: 120))
        XCTAssertEqual(payload["period_s"] as? Int, 120)
        XCTAssertEqual(payload["trigger"] as? String, "timer")

        let clap = DuckController.DuckRule(
            trigger: "clap",
            action: "skill",
            value: "roulade",
            periodS: 120
        )
        XCTAssertNil(
            DuckController.ruleWireObject(clap)["period_s"],
            "the core reads a period only on a timer (rules.rs:160-164)"
        )
    }

    func testShareThenParseKeepsTimerPeriod() {
        let rule = timer(period: 60)
        let shared = DuckController.shareText([rule])
        XCTAssertTrue(
            shared.contains("\"period_s\":60"),
            "the share document must carry the timer's period"
        )
        let imported = DuckController.parseSharedRules(shared)
        XCTAssertEqual(imported?.rules ?? [], [rule])
        XCTAssertEqual(imported?.skippedTimers ?? -1, 0)
    }

    func testChainAndPeriodSurviveTogether() {
        let rule = DuckController.DuckRule(
            trigger: "timer",
            action: "skill",
            value: "roulade",
            then: [DuckController.DuckRuleStep(action: "emotion", value: "laugh")],
            periodS: 300
        )
        let shared = DuckController.shareText([rule])
        XCTAssertEqual(
            DuckController.parseSharedRules(shared)?.rules ?? [],
            [rule]
        )
    }

    func testPasteVocabularyIsTheCoresTriggers() {
        XCTAssertEqual(
            DuckController.ruleTriggers.sorted(),
            ["clap", "pet", "timer", "whistle"],
            "the paste door must know every trigger the core accepts"
        )
    }

    func testTimerWithoutAPeriodIsCountedNotImported() {
        let text = document(#"[{"trigger":"timer","action":"skill","value":"roulade"}]"#)
        let imported = DuckController.parseSharedRules(text)
        XCTAssertEqual(imported?.rules.count ?? -1, 0)
        XCTAssertEqual(
            imported?.skippedTimers ?? -1,
            1,
            "the core refuses a timer with no period; the sheet must say so"
        )
    }

    func testOutOfRangePeriodsAreCountedNotImported() {
        for period in [4, 3601] {
            let text = document(
                #"[{"trigger":"timer","action":"skill","value":"roulade","period_s":\#(period)}]"#
            )
            let imported = DuckController.parseSharedRules(text)
            XCTAssertEqual(
                imported?.rules.count ?? -1,
                0,
                "period_s \(period) is outside the core's 5...3600 window"
            )
            XCTAssertEqual(imported?.skippedTimers ?? -1, 1)
        }
    }

    func testFractionalPeriodIsCountedNotImported() {
        let text = document(
            #"[{"trigger":"timer","action":"skill","value":"roulade","period_s":60.5}]"#
        )
        let imported = DuckController.parseSharedRules(text)
        XCTAssertEqual(imported?.rules.count ?? -1, 0)
        XCTAssertEqual(imported?.skippedTimers ?? -1, 1)
    }

    func testOneBadTimerDoesNotDiscardTheLegalBlocks() {
        let text = document(
            #"[{"trigger":"clap","action":"skill","value":"roulade"},{"trigger":"timer","action":"skill","value":"sit","period_s":4}]"#
        )
        let imported = DuckController.parseSharedRules(text)
        XCTAssertEqual(imported?.rules.map { $0.trigger } ?? [], ["clap"])
        XCTAssertEqual(imported?.skippedTimers ?? -1, 1)
    }

    func testNonTimerPeriodIsIgnored() {
        let text = document(
            #"[{"trigger":"clap","action":"skill","value":"roulade","period_s":60}]"#
        )
        let imported = DuckController.parseSharedRules(text)
        XCTAssertEqual(imported?.rules.count ?? -1, 1)
        XCTAssertEqual(imported?.rules.first?.trigger, "clap")
        XCTAssertNil(
            imported?.rules.first?.periodS,
            "period_s is a timer's field; on any other trigger it carries no meaning"
        )
    }

    func testUnknownTriggerIsDropped() {
        let text = document(
            #"[{"trigger":"snap","action":"skill","value":"roulade"}]"#
        )
        let imported = DuckController.parseSharedRules(text)
        XCTAssertEqual(imported?.rules.count ?? -1, 0)
        XCTAssertEqual(imported?.skippedTimers ?? -1, 0)
    }

    func testPlainTextIsNotAProgram() {
        XCTAssertNil(DuckController.parseSharedRules("hello duck"))
    }
}

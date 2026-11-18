import XCTest
@testable import Duck

/// The collection's badges (G2): every threshold is evaluated against the
/// real `/collection` numbers. The edges are pinned here — 12 vs 13 moods,
/// 9 vs 10 and 999 vs 1000 moments, an empty manifest — because a badge is
/// a claim about the duck, and a wrong one is a lie.
final class CollectionBadgeTests: XCTestCase {

    private func book(
        moods: [String: Int],
        tricks: [String: Int] = [:],
        moodsTotal: Int = 13,
        tricksTotal: Int? = nil
    ) -> CollectionBook {
        let emotions = moods.keys.sorted().map {
            CollectionEntry(name: $0, count: moods[$0] ?? 0, firstDay: "")
        }
        let skillRows = tricks.keys.sorted().map {
            CollectionEntry(name: $0, count: tricks[$0] ?? 0, firstDay: "")
        }
        return CollectionBook(
            emotions: emotions,
            tricks: skillRows,
            moodsUnlocked: emotions.filter { !$0.isLocked }.count,
            moodsTotal: moodsTotal,
            tricksUnlocked: skillRows.filter { !$0.isLocked }.count,
            tricksTotal: tricksTotal ?? skillRows.count
        )
    }

    private func badge(
        _ badges: [CollectionBadge],
        _ kind: CollectionBadgeKind
    ) -> CollectionBadge {
        guard let found = badges.first(where: { $0.kind == kind }) else {
            XCTFail("missing badge \(kind.rawValue)")
            return CollectionBadge(kind: kind, unlocked: false, progress: 0, goal: 0)
        }
        return found
    }

    func testFreshBookLocksAllNineBadgesWithoutInventingNumbers() {
        let badges = CollectionBadges.evaluate(book: book(moods: [:], tricks: [:]))
        XCTAssertEqual(badges.count, 9, "the ticket's four badges plus five milestones")
        XCTAssertEqual(badges.map(\.kind), CollectionBadgeKind.allCases)
        XCTAssertTrue(badges.allSatisfy { !$0.unlocked })
        XCTAssertTrue(badges.allSatisfy { $0.progress == 0 })
        XCTAssertEqual(badge(badges, .allMoods).goal, 13)
        XCTAssertEqual(badge(badges, .allMoods).progressText, "0/13")
        XCTAssertEqual(badge(badges, .allTricks).goal, 0)
        XCTAssertEqual(badge(badges, .allTricks).progressText, "0/0")
    }

    func testFirstRealCountUnlocksTheFirstBadges() {
        let badges = CollectionBadges.evaluate(book: book(
            moods: ["curious": 1],
            tricks: ["roulade": 1]
        ))
        XCTAssertTrue(badge(badges, .firstMood).unlocked)
        XCTAssertEqual(badge(badges, .firstMood).progressText, "1/1")
        XCTAssertTrue(badge(badges, .firstTrick).unlocked)
        XCTAssertEqual(badge(badges, .firstTrick).progressText, "1/1")
        XCTAssertFalse(badge(badges, .allMoods).unlocked)
        XCTAssertEqual(badge(badges, .allMoods).progressText, "1/13")
        // One loaded trick, and that one is unlocked: the loaded set is
        // the goal the core publishes, so the school is finished.
        XCTAssertTrue(badge(badges, .allTricks).unlocked)
        XCTAssertEqual(badge(badges, .allTricks).progressText, "1/1")
    }

    func testAllMoodsNeedsThirteenNotJustEveryLoadedRow() {
        var five: [String: Int] = [:]
        for index in 0..<5 { five["mood\(index)"] = 1 }
        let partial = CollectionBadges.evaluate(book: book(moods: five, moodsTotal: 5))
        XCTAssertFalse(
            badge(partial, .allMoods).unlocked,
            "five moods loaded is not the official thirteen"
        )
        XCTAssertEqual(badge(partial, .allMoods).goal, 13)
        XCTAssertEqual(badge(partial, .allMoods).progressText, "5/13")

        var twelve: [String: Int] = [:]
        for index in 0..<12 { twelve["mood\(index)"] = 1 }
        let almost = CollectionBadges.evaluate(book: book(moods: twelve, moodsTotal: 13))
        XCTAssertFalse(badge(almost, .allMoods).unlocked)
        XCTAssertEqual(badge(almost, .allMoods).progressText, "12/13")

        var thirteen: [String: Int] = [:]
        for index in 0..<13 { thirteen["mood\(index)"] = 1 }
        let all = CollectionBadges.evaluate(book: book(moods: thirteen, moodsTotal: 13))
        XCTAssertTrue(badge(all, .allMoods).unlocked)
        XCTAssertEqual(badge(all, .allMoods).progressText, "13/13")
    }

    func testMoodMilestoneBoundaries() {
        let cases: [(Int, Bool, Bool, Bool)] = [
            (9, false, false, false),
            (10, true, false, false),
            (99, true, false, false),
            (100, true, true, false),
            (999, true, true, false),
            (1000, true, true, true),
        ]
        for (total, ten, hundred, thousand) in cases {
            let badges = CollectionBadges.evaluate(book: book(moods: ["angry": total]))
            XCTAssertEqual(badge(badges, .moods10).unlocked, ten, "moods \(total)")
            XCTAssertEqual(badge(badges, .moods100).unlocked, hundred, "moods \(total)")
            XCTAssertEqual(badge(badges, .moods1000).unlocked, thousand, "moods \(total)")
        }
    }

    func testMilestonesSumRealCountsAndDisplayCapsAtTheGoal() {
        let badges = CollectionBadges.evaluate(book: book(moods: ["angry": 128]))
        XCTAssertEqual(badge(badges, .moods10).progressText, "10/10")
        XCTAssertEqual(badge(badges, .moods100).progressText, "100/100")
        XCTAssertEqual(badge(badges, .moods1000).progressText, "128/1000")
        XCTAssertFalse(badge(badges, .moods1000).unlocked)

        let past = CollectionBadges.evaluate(book: book(moods: ["angry": 1001]))
        XCTAssertTrue(badge(past, .moods1000).unlocked)
        XCTAssertEqual(
            badge(past, .moods1000).progressText,
            "1000/1000",
            "a count past the goal still reads as the goal"
        )
        XCTAssertEqual(
            badge(past, .allMoods).progressText,
            "1/13",
            "one mood done a thousand times is still one mood"
        )
    }

    func testTrickMilestoneBoundaries() {
        let cases: [(Int, Bool, Bool)] = [
            (9, false, false),
            (10, true, false),
            (99, true, false),
            (100, true, true),
        ]
        for (total, ten, hundred) in cases {
            let badges = CollectionBadges.evaluate(book: book(
                moods: [:],
                tricks: ["roulade": total]
            ))
            XCTAssertEqual(badge(badges, .tricks10).unlocked, ten, "tricks \(total)")
            XCTAssertEqual(badge(badges, .tricks100).unlocked, hundred, "tricks \(total)")
        }
    }

    func testAFinishedBookUnlocksEveryBadge() {
        var moods: [String: Int] = [:]
        for index in 0..<13 { moods["mood\(index)"] = 1000 }
        let badges = CollectionBadges.evaluate(book: book(
            moods: moods,
            tricks: ["roulade": 100, "kick_left": 1]
        ))
        XCTAssertTrue(badges.allSatisfy { $0.unlocked })
        XCTAssertEqual(
            badges.map(\.progressText),
            [
                "1/1", "13/13", "1/1", "2/2",
                "10/10", "100/100", "1000/1000", "10/10", "100/100",
            ]
        )
    }

    func testAllTricksNeedsALoadedSetAndEveryOneOfIt() {
        let none = CollectionBadges.evaluate(book: book(moods: [:], tricks: [:]))
        XCTAssertFalse(badge(none, .allTricks).unlocked)
        XCTAssertEqual(badge(none, .allTricks).progressText, "0/0")

        let partial = CollectionBadges.evaluate(book: book(
            moods: [:],
            tricks: ["roulade": 3, "kick_left": 0],
            tricksTotal: 2
        ))
        XCTAssertFalse(badge(partial, .allTricks).unlocked)
        XCTAssertEqual(badge(partial, .allTricks).progressText, "1/2")

        let all = CollectionBadges.evaluate(book: book(
            moods: [:],
            tricks: ["roulade": 1, "kick_left": 1],
            tricksTotal: 2
        ))
        XCTAssertTrue(badge(all, .allTricks).unlocked)
        XCTAssertEqual(badge(all, .allTricks).progressText, "2/2")
    }

    func testBadgeIdsAreTheCrossShellKebabIds() {
        XCTAssertEqual(
            CollectionBadgeKind.allCases.map(\.id),
            [
                "first-mood", "all-moods", "first-trick", "all-tricks",
                "moods-10", "moods-100", "moods-1000", "tricks-10", "tricks-100",
            ],
            "Android's shelf uses the same ids, so the two phones' tests read alike"
        )
        XCTAssertEqual(Set(CollectionBadgeKind.allCases.map(\.id)).count, 9)
    }

    func testDuplicatedRowsNeitherUnlockNorDoubleCount() {
        var rows = (0..<12).map {
            CollectionEntry(name: "mood\($0)", count: 1, firstDay: "")
        }
        // Thirteen rows, twelve names: repetition is not a new mood.
        rows.append(CollectionEntry(name: "mood0", count: 1, firstDay: ""))
        let duplicated = CollectionBook(
            emotions: rows,
            tricks: [],
            moodsUnlocked: 13,
            moodsTotal: 13,
            tricksUnlocked: 0,
            tricksTotal: 0
        )
        let badges = CollectionBadges.evaluate(book: duplicated)
        XCTAssertFalse(badge(badges, .allMoods).unlocked)
        XCTAssertEqual(badge(badges, .allMoods).progress, 12)
        XCTAssertEqual(badge(badges, .moods10).progress, 12)
    }

    func testConditionLineStatesTheRealThreshold() {
        let badges = CollectionBadges.evaluate(book: book(
            moods: ["angry": 7],
            tricks: ["roulade": 1]
        ))
        XCTAssertTrue(badge(badges, .firstMood).conditionText.contains("1"))
        XCTAssertTrue(badge(badges, .allMoods).conditionText.contains("13/13"))
        XCTAssertTrue(badge(badges, .moods10).conditionText.contains("10"))
        XCTAssertTrue(badge(badges, .moods1000).conditionText.contains("1000"))
        XCTAssertTrue(badge(badges, .allTricks).conditionText.contains("1/1"))
    }
}

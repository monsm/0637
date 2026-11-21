import Foundation
import XCTest
@testable import Duck

/// The collection book's pure half (F2): the `/collection` contract becomes
/// rows, dates and totals. An em dash is a real answer ("day unknown"), a
/// locked row is exactly `count == 0`, and nothing is ever guessed.
final class CollectionBookTests: XCTestCase {

    func testUnknownFirstDayIsAnEmDash() {
        let entry = CollectionEntry(name: "roulade", count: 5, firstDay: "")
        XCTAssertEqual(entry.firstDayText, "—")
        XCTAssertFalse(entry.isLocked)
        XCTAssertEqual(
            CollectionEntry(name: "x", count: 1, firstDay: "  ").firstDayText,
            "—",
            "whitespace is still an unknown day, never a date"
        )
    }

    func testKnownFirstDayIsShownVerbatim() {
        let entry = CollectionEntry(
            name: "curious",
            count: 3,
            firstDay: "2026-11-18"
        )
        XCTAssertEqual(entry.firstDayText, "2026-11-18")
        XCTAssertEqual(entry.countText, "3")
    }

    func testLockedEntryHasZeroNoDateAndIsLocked() {
        let entry = CollectionEntry(
            name: "sad",
            count: 0,
            firstDay: "2026-11-18"
        )
        XCTAssertTrue(entry.isLocked)
        XCTAssertEqual(entry.countText, "0")
        XCTAssertNil(
            entry.firstDayText,
            "a locked row has no date, even if the wire sends one"
        )
    }

    func testUnlockedEntriesSortBeforeLockedThenByCountThenName() {
        let entries = [
            CollectionEntry(name: "angry", count: 0, firstDay: ""),
            CollectionEntry(name: "roulade", count: 5, firstDay: ""),
            CollectionEntry(name: "sad", count: 3, firstDay: ""),
            CollectionEntry(name: "curious", count: 3, firstDay: ""),
            CollectionEntry(name: "laugh", count: 0, firstDay: ""),
        ]
        XCTAssertEqual(
            CollectionBook.ordered(entries).map(\.name),
            ["roulade", "curious", "sad", "angry", "laugh"],
            "unlocked first, then count, then the official name"
        )
    }

    func testParseFollowsTheContract() throws {
        let json: [String: Any] = [
            "ok": true,
            "emotions": [
                ["name": "curious", "count": 3, "first_day": "2026-11-18"] as [String: Any],
                ["name": "sad", "count": 0, "first_day": ""] as [String: Any],
            ],
            "skills": [
                ["name": "roulade", "count": 5, "first_day": ""] as [String: Any],
            ],
            "moods": ["unlocked": 3, "total": 13],
            "tricks": ["unlocked": 2, "total": 4],
        ]
        let book = try XCTUnwrap(CollectionBook.parse(json))
        XCTAssertEqual(book.moodsUnlocked, 3)
        XCTAssertEqual(book.moodsTotal, 13)
        XCTAssertEqual(book.tricksUnlocked, 2)
        XCTAssertEqual(book.tricksTotal, 4)
        XCTAssertEqual(book.emotions.map(\.name), ["curious", "sad"])
        XCTAssertEqual(book.tricks.map(\.name), ["roulade"])
        XCTAssertEqual(book.tricks.first?.firstDayText, "—")
        XCTAssertNil(try XCTUnwrap(book.emotions.last).firstDayText)
        XCTAssertEqual(book.emotions.first?.countText, "3")
        XCTAssertEqual(book.emotions.last?.countText, "0")
    }

    func testCoreTotalsWinOverWhatTheRowsShow() {
        // The list can be shorter than the core's own totals; the header
        // must report the core's numbers, not a recount of this page.
        let json: [String: Any] = [
            "emotions": [
                ["name": "curious", "count": 3, "first_day": "2026-11-18"] as [String: Any],
            ],
            "skills": [[String: Any]](),
            "moods": ["unlocked": 7, "total": 13],
            "tricks": ["unlocked": 2, "total": 4],
        ]
        let book = CollectionBook.parse(json)
        XCTAssertEqual(book?.moodsUnlocked, 7)
        XCTAssertEqual(book?.moodsTotal, 13)
        XCTAssertEqual(book?.tricksUnlocked, 2)
        XCTAssertEqual(book?.tricksTotal, 4)
    }

    func testMissingTotalsFallBackToTheRows() {
        let json: [String: Any] = [
            "emotions": [
                ["name": "curious", "count": 2, "first_day": ""] as [String: Any],
            ],
            "skills": [[String: Any]](),
        ]
        let book = CollectionBook.parse(json)
        XCTAssertEqual(book?.moodsUnlocked, 1)
        XCTAssertEqual(book?.moodsTotal, 1)
        XCTAssertEqual(book?.tricksUnlocked, 0)
        XCTAssertEqual(book?.tricksTotal, 0)
    }

    func testMalformedDocumentIsNotGuessed() {
        XCTAssertNil(CollectionBook.parse([String: Any]()))
        let missingSkills: [String: Any] = ["emotions": [[String: Any]]()]
        XCTAssertNil(CollectionBook.parse(missingSkills))
        let refused: [String: Any] = [
            "ok": false,
            "emotions": [[String: Any]](),
            "skills": [[String: Any]](),
        ]
        XCTAssertNil(CollectionBook.parse(refused), "ok:false is not a book")
        XCTAssertNil(CollectionBook.parse(Data("not json".utf8)))
    }

    func testNamelessEntryIsDroppedNotInvented() {
        let json: [String: Any] = [
            "emotions": [
                ["count": 2, "first_day": ""] as [String: Any],
                ["name": "yes", "count": 1, "first_day": "2026-11-18"] as [String: Any],
            ],
            "skills": [[String: Any]](),
        ]
        XCTAssertEqual(CollectionBook.parse(json)?.emotions.map(\.name), ["yes"])
    }

    func testNegativeCountIsClampedToLockedZero() throws {
        let json: [String: Any] = [
            "emotions": [
                ["name": "no", "count": -3, "first_day": ""] as [String: Any],
            ],
            "skills": [[String: Any]](),
        ]
        let entry = try XCTUnwrap(CollectionBook.parse(json)?.emotions.first)
        XCTAssertEqual(entry.count, 0)
        XCTAssertEqual(entry.isLocked, true)
        XCTAssertNil(entry.firstDayText)
    }

    func testEmptyFirstDayFromJSONRendersTheDash() throws {
        let json: [String: Any] = [
            "emotions": [
                ["name": "curious", "count": 3, "first_day": ""] as [String: Any],
            ],
            "skills": [[String: Any]](),
        ]
        let entry = try XCTUnwrap(CollectionBook.parse(json)?.emotions.first)
        XCTAssertEqual(entry.firstDayText, "—")
    }

    func testWireNamesUseTheStageLabel() {
        XCTAssertEqual(
            SkillLabel.of("kick_left", language: .english),
            "kick left",
            "English must not show a protocol underscore"
        )
        XCTAssertEqual(SkillLabel.of("roulade", language: .chinese), "翻滚")
        XCTAssertEqual(SkillLabel.of("curious", language: .chinese), "好奇")
        XCTAssertEqual(SkillLabel.of("yes_fast", language: .chinese), "快速点头")
        XCTAssertEqual(
            SkillLabel.of("custom_walk", language: .chinese),
            "custom_walk",
            "an unknown manifest skill keeps its real wire name"
        )
    }
}

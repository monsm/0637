import Foundation
@testable import Duck
import XCTest

/// The days board (K2, 2026-11-19): `GET /days` is the core's own document.
/// These tests pin the pure reading — missing counters are 0, an absent
/// today stays absent, broken JSON is refused, and the shared text contains
/// only dates the core actually sent. No screen, no core, no clock.
final class DaysBoardTests: XCTestCase {

    private func board(_ json: String) -> DaysBoard? {
        DaysBoard.parse(Data(json.utf8))
    }

    private func dates(in text: String) -> [String] {
        guard let regex = try? NSRegularExpression(pattern: #"\d{4}-\d{2}-\d{2}"#) else {
            return []
        }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        let found = regex.matches(in: text, range: range).compactMap { match in
            Range(match.range, in: text).map { String(text[$0]) }
        }
        return Array(Set(found)).sorted()
    }

    // MARK: - Parsing

    func testParsesCompletedDayTodayAndWeek() throws {
        let json = #"""
        {"ok":true,
         "days":[{"day":"2026-11-18","metres":12.3,"powered_s":600.0,"rolls":2,
                  "pets":5,"pickups":1,"falls":0,"fed":2,"claps":3,"whistles":1,
                  "ground_picks":0,"sit_downs":1}],
         "today":{"day":"2026-11-19","metres":1.2,"powered_s":120.0,"rolls":0,
                  "pets":1,"pickups":0,"falls":0,"fed":0,"claps":0,"whistles":0,
                  "ground_picks":0,"sit_downs":0},
         "week":{"metres":13.5,"powered_s":720.0,"rolls":2,"pets":6,"pickups":1,
                 "falls":0,"fed":2,"claps":3,"whistles":1}}
        """#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertFalse(parsed.isEmpty)
        XCTAssertEqual(parsed.days.count, 1)
        XCTAssertEqual(parsed.days.first?.day, "2026-11-18")
        XCTAssertEqual(parsed.days.first?.metres ?? -1, 12.3, accuracy: 0.0001)
        XCTAssertEqual(parsed.days.first?.poweredS ?? -1, 600.0, accuracy: 0.0001)
        XCTAssertEqual(parsed.days.first?.rolls, 2)
        XCTAssertEqual(parsed.days.first?.pets, 5)
        XCTAssertEqual(parsed.days.first?.pickups, 1)
        XCTAssertEqual(parsed.days.first?.falls, 0)
        XCTAssertEqual(parsed.days.first?.fed, 2)
        XCTAssertEqual(parsed.days.first?.claps, 3)
        XCTAssertEqual(parsed.days.first?.whistles, 1)
        XCTAssertEqual(parsed.days.first?.groundPicks, 0)
        XCTAssertEqual(parsed.days.first?.sitDowns, 1)
        XCTAssertEqual(parsed.today?.day, "2026-11-19")
        XCTAssertEqual(parsed.today?.metres ?? -1, 1.2, accuracy: 0.0001)
        XCTAssertEqual(parsed.week?.metres ?? -1, 13.5, accuracy: 0.0001)
        XCTAssertEqual(parsed.week?.pets, 6)
    }

    func testMissingCountersReadAsZero() throws {
        // An older save may carry only the day and metres. Every other key
        // is 0, not a guess and not a gap.
        let json = #"{"ok":true,"days":[{"day":"2026-11-01","metres":3.0}],"week":{"metres":3.0}}"#
        let parsed = try XCTUnwrap(board(json))
        let day = try XCTUnwrap(parsed.days.first)
        XCTAssertEqual(day.rolls, 0)
        XCTAssertEqual(day.pets, 0)
        XCTAssertEqual(day.pickups, 0)
        XCTAssertEqual(day.falls, 0)
        XCTAssertEqual(day.fed, 0)
        XCTAssertEqual(day.claps, 0)
        XCTAssertEqual(day.whistles, 0)
        XCTAssertEqual(day.groundPicks, 0)
        XCTAssertEqual(day.sitDowns, 0)
        XCTAssertEqual(day.poweredS, 0, accuracy: 0.0001)
        XCTAssertEqual(parsed.week?.rolls, 0)
        XCTAssertEqual(parsed.week?.metres ?? -1, 3.0, accuracy: 0.0001)
    }

    func testDayRowSummaryCarriesAllNineLabelsInOrder() throws {
        // The day rows carry the same nine counters as the week card, the
        // share text and Android's daysLines — 陪伴、米、翻滚、被摸、被抱起、
        // 摔倒、被喂、拍手、口哨.
        let json = #"""
        {"ok":true,"days":[{"day":"2026-11-18","metres":12.3,"powered_s":600.0,
                 "rolls":2,"pets":5,"pickups":1,"falls":0,"fed":2,"claps":3,
                 "whistles":1}]}
        """#
        let day = try XCTUnwrap(board(json)?.days.first)
        let labels = [
            Copy.daysPowered, Copy.daysMetres, Copy.daysRolls,
            Copy.daysPets, Copy.daysPickups, Copy.daysFalls,
            Copy.daysFed, Copy.daysClaps, Copy.daysWhistles,
        ]
        var cursor = day.summary.startIndex
        for label in labels {
            guard let found = day.summary.range(
                of: label,
                range: cursor..<day.summary.endIndex
            ) else {
                XCTFail("the day row must show '\(label)', like Android's daysLines")
                return
            }
            cursor = found.upperBound
        }
    }

    func testBrokenJsonIsNil() {
        XCTAssertNil(board("not json"))
        XCTAssertNil(board(#""a bare string""#))
        XCTAssertNil(board("{"))
    }

    func testAShapeWithoutDaysIsNil() {
        // The contract always carries `days`; anything else is not this
        // endpoint's answer, and the sheet must not draw a board from it.
        XCTAssertNil(board(#"{"ok":true}"#))
        XCTAssertNil(board(#"{"ok":true,"days":{}}"#))
        XCTAssertNil(board(#"[{"day":"2026-11-01"}]"#))
        XCTAssertNil(board(#"{"ok":false,"days":[]}"#))
    }

    // MARK: - Absences

    func testEmptyBoardHasNoWeekCardAndNoZeros() throws {
        let parsed = try XCTUnwrap(board(#"{"ok":true,"days":[]}"#))
        XCTAssertTrue(parsed.isEmpty)
        XCTAssertNil(parsed.today)
        XCTAssertNil(parsed.week, "no days and no core week means no card, not a row of 0s")
        XCTAssertEqual(parsed.shareText(), "")
    }

    func testTodayAbsentMeansNoTodayRow() throws {
        let json = #"{"ok":true,"days":[{"day":"2026-11-18","rolls":2}]}"#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertNil(parsed.today, "the core sent no today; the board invents none")
        XCTAssertFalse(parsed.isEmpty)
        XCTAssertEqual(parsed.days.count, 1)
    }

    func testDayWithoutADateIsDroppedNotNamed() throws {
        let json = #"""
        {"ok":true,"days":[{"metres":5.0},{"day":"","rolls":1},{"day":"   ","rolls":1},{"day":"2026-11-05","rolls":1}]}
        """#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertEqual(parsed.days.map(\.day), ["2026-11-05"])
    }

    // MARK: - The week's totals

    func testTheCoreWeekTotalsAreUsedAsPublished() throws {
        // The core sends the week; the shell must not re-sum the days and
        // silently replace it (the two can differ: days cover real history).
        let json = #"""
        {"ok":true,"days":[{"day":"2026-11-18","metres":2.0}],"week":{"metres":9.9,"rolls":7}}
        """#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertEqual(parsed.week?.metres ?? -1, 9.9, accuracy: 0.0001)
        XCTAssertEqual(parsed.week?.rolls, 7)
    }

    func testWithoutACoreWeekNoCardIsInvented() throws {
        // No week object: no card. Summing the days would claim a weekly
        // total the core never published (and today is not a finished day).
        let json = #"""
        {"ok":true,
         "days":[{"day":"2026-11-17","metres":1.5,"rolls":1},
                 {"day":"2026-11-18","metres":2.5,"rolls":2}],
         "today":{"day":"2026-11-19","metres":100.0,"rolls":9}}
        """#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertNil(parsed.week)
        XCTAssertTrue(parsed.shareText().contains("2026-11-18"))
        XCTAssertFalse(
            parsed.shareText().contains("This week / 这一周"),
            "a missing week block is hidden, not replaced by a shell sum"
        )
    }

    // MARK: - The shared document

    func testShareTextCarriesOnlyDatesTheCoreSent() throws {
        let json = #"""
        {"ok":true,
         "days":[{"day":"2026-11-18","metres":12.3}],
         "today":{"day":"2026-11-19","metres":1.2},
         "week":{"metres":13.5}}
        """#
        let parsed = try XCTUnwrap(board(json))
        let text = parsed.shareText()
        XCTAssertEqual(
            dates(in: text),
            ["2026-11-18", "2026-11-19"],
            "the share text must not name a day the core never sent"
        )
    }

    func testShareTextIsBilingual() throws {
        let json = #"""
        {"ok":true,"days":[{"day":"2026-11-18","metres":12.3,"pets":5}],
         "week":{"metres":12.3}}
        """#
        let text = try XCTUnwrap(board(json)).shareText()
        XCTAssertTrue(text.contains("This week / 这一周"), "the week title is said in both languages")
        XCTAssertTrue(text.contains("Metres: 12.3 m / 米: 12.3 m"), "each fact line carries EN and ZH")
        XCTAssertTrue(text.contains("Pets: 5 / 被摸: 5"))
    }

    func testShareTextRestoresTheInterfaceLanguage() {
        let saved = Copy.language
        defer { Copy.language = saved }
        Copy.language = .chinese
        let json = #"{"ok":true,"days":[{"day":"2026-11-18","metres":12.3}]}"#
        _ = board(json)?.shareText()
        XCTAssertEqual(Copy.language, .chinese, "shareText flips the language to render twice and must put it back")
    }

    func testTodaySharesTheCoresDateVerbatim() throws {
        // No clock anywhere: an odd but real date is kept exactly.
        let json = #"{"ok":true,"days":[],"today":{"day":"2025-01-02","metres":0.5}}"#
        let parsed = try XCTUnwrap(board(json))
        XCTAssertFalse(parsed.isEmpty)
        XCTAssertTrue(parsed.shareText().contains("Today (2025-01-02) / 今天 (2025-01-02)"))
    }

    func testEmptyBoardShareTextHasNoDatesAtAll() throws {
        let parsed = try XCTUnwrap(board(#"{"ok":true,"days":[]}"#))
        XCTAssertTrue(dates(in: parsed.shareText()).isEmpty)
    }

    func testPoweredTimeIsRealHours() {
        XCTAssertEqual(DaysDay.powered(600.0), "0.2 h")
        XCTAssertEqual(DaysDay.powered(720.0), "0.2 h")
        XCTAssertEqual(DaysDay.powered(3600.0), "1.0 h")
    }
}

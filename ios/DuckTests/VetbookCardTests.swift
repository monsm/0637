import XCTest
@testable import Duck

/// The card's mapping is the zero-fabrication rule in code: a number the
/// document does not carry stays off the card, a life with nothing in it
/// reads "no records yet" instead of a grid of invented zeroes, and the
/// build fingerprint is cut to the eight digits the card prints. The
/// document shape is the core's `GET /export/vetbook.json` (export.rs).
final class VetbookCardTests: XCTestCase {
    /// A document in exactly the core's shape; sections stay out when the
    /// test does not hand them in, so "missing" really is missing.
    private func document(
        highlights: [String: Any] = [:],
        care: [String: Any] = [:],
        life: [String: Any] = [:],
        fingerprint: String = ""
    ) -> [String: Any] {
        var json: [String: Any] = ["format": "microduck-vetbook/1"]
        if !highlights.isEmpty { json["highlights"] = highlights }
        if !care.isEmpty { json["care"] = care }
        if !life.isEmpty { json["life"] = life }
        if !fingerprint.isEmpty { json["build"] = ["fingerprint": fingerprint] }
        return json
    }

    func testTheCoreDocumentMapsToTheCardNumbers() {
        let model = VetbookCardParser.parse(document(
            highlights: [
                "metres": 123.4, "rolls": 3, "pickups": 5, "falls": 1,
                "pets": 7, "fed": 4, "claps": 2, "whistles": 1,
            ],
            care: ["hunger": 0.25, "mood": 0.5, "bond": 0.8],
            life: ["powered_s": 7200.0],
            fingerprint: "0123456789abcdef"
        ))
        XCTAssertEqual(model.metres ?? 0, 123.4, accuracy: 0.001)
        XCTAssertEqual(model.rolls, 3)
        XCTAssertEqual(model.pickups, 5)
        XCTAssertEqual(model.falls, 1)
        XCTAssertEqual(model.pets, 7)
        XCTAssertEqual(model.fed, 4)
        XCTAssertEqual(model.claps, 2)
        XCTAssertEqual(model.whistles, 1)
        XCTAssertEqual(model.poweredHours ?? 0, 2.0, accuracy: 0.001)
        XCTAssertEqual(model.fullness, 75)
        XCTAssertEqual(model.mood, 75)
        XCTAssertEqual(model.bond, 80)
        XCTAssertEqual(model.fingerprint, "01234567")
        XCTAssertFalse(model.isEmpty)
    }

    func testAnEmptyLifeIsEmptyEvenWithPoweredSeconds() {
        let model = VetbookCardParser.parse(document(
            highlights: [
                "metres": 0.0, "rolls": 0, "pickups": 0, "falls": 0,
                "pets": 0, "fed": 0, "claps": 0, "whistles": 0,
            ],
            care: ["hunger": 0.0, "mood": 0.0, "bond": 0.0],
            life: ["powered_s": 30.0]
        ))
        XCTAssertTrue(model.isEmpty)
        // The powered time itself is real and still mapped; the empty card
        // just declines to dress it up as a record.
        XCTAssertEqual(model.poweredHours ?? 0, 30.0 / 3600.0, accuracy: 0.0001)
    }

    func testMissingFieldsStayMissing() {
        let model = VetbookCardParser.parse(document())
        XCTAssertNil(model.metres)
        XCTAssertNil(model.rolls)
        XCTAssertNil(model.pickups)
        XCTAssertNil(model.falls)
        XCTAssertNil(model.pets)
        XCTAssertNil(model.fed)
        XCTAssertNil(model.claps)
        XCTAssertNil(model.whistles)
        XCTAssertNil(model.poweredHours)
        XCTAssertNil(model.fullness)
        XCTAssertNil(model.mood)
        XCTAssertNil(model.bond)
        XCTAssertNil(model.fingerprint)
        XCTAssertTrue(model.isEmpty)
    }

    func testOneRealRecordIsNotEmptyEvenBesideZeros() {
        let model = VetbookCardParser.parse(document(
            highlights: [
                "metres": 0.0, "rolls": 0, "pickups": 2, "falls": 0,
                "pets": 0, "fed": 0, "claps": 0, "whistles": 0,
            ]
        ))
        XCTAssertFalse(model.isEmpty)
        XCTAssertEqual(model.pickups, 2)
    }

    /// Care percentages keep the core's own derivations and cannot leave
    /// their honest 0–100 range, whatever a bad reading claims.
    func testCarePercentagesDeriveAndClamp() {
        let model = VetbookCardParser.parse(document(
            care: ["hunger": 0.25, "mood": -1.0, "bond": 0.0]
        ))
        XCTAssertEqual(model.fullness, 75)
        XCTAssertEqual(model.mood, 0)
        XCTAssertEqual(model.bond, 0)

        let clamped = VetbookCardParser.parse(document(
            care: ["hunger": -0.5, "mood": 99.0, "bond": 2.0]
        ))
        XCTAssertEqual(clamped.fullness, 100)
        XCTAssertEqual(clamped.mood, 100)
        XCTAssertEqual(clamped.bond, 100)
    }
}

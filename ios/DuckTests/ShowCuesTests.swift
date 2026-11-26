import XCTest
@testable import Duck

/// The one-duck show: a long idle earns one three-beat routine, and every
/// beat is a verb the duck already has.
final class ShowCuesTests: XCTestCase {
    func testTheShowHoldsUntilTheLongIdle() {
        XCTAssertFalse(showDue(idleS: 239.9, enabled: true, done: false))
    }

    func testALongIdleEarnsOneShow() {
        XCTAssertTrue(showDue(idleS: 240, enabled: true, done: false))
        XCTAssertFalse(showDue(idleS: 300, enabled: true, done: true), "a second show needs a new long idle")
    }

    func testTheSwitchMustBeOn() {
        XCTAssertFalse(showDue(idleS: 300, enabled: false, done: false))
    }

    func testTheBeatsAreTheDucksOwnStandingVerbs() {
        XCTAssertEqual(showBeats, ["roulade", "curious", "kick_left"])
    }
}

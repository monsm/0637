import XCTest
@testable import Duck

/// The two settling sounds before the doze: a coo at 600 derived idle
/// seconds, a chirp at 690, each once per spell, and both before care.rs'
/// 720 s doze.
final class SleepCuesTests: XCTestCase {
    func testSilenceBeforeTheFirstMark() {
        XCTAssertNil(sleepCue(idleS: 599.9, cooed: false, chirped: false))
    }

    func testASettlingCooAtSixHundred() {
        XCTAssertEqual(sleepCue(idleS: 600, cooed: false, chirped: false), "coo")
    }

    func testTheCooIsOncePerSpell() {
        XCTAssertNil(sleepCue(idleS: 650, cooed: true, chirped: false))
    }

    func testAQuieterChirpAtSixNinety() {
        XCTAssertEqual(sleepCue(idleS: 690, cooed: true, chirped: false), "chirp")
    }

    func testTheChirpIsOncePerSpell() {
        XCTAssertNil(sleepCue(idleS: 700, cooed: true, chirped: true))
    }

    func testAJumpPastBothKeepsTheLaterSound() {
        // The app was away while the clock ran on: the coo's moment has
        // passed, and claiming it late would be a lie about the present.
        XCTAssertEqual(sleepCue(idleS: 700, cooed: false, chirped: false), "chirp")
    }

    func testThePublishedClockIsScaledLikeTheCoreDoze() {
        // /state.care.idle_s is published unscaled; care.rs multiplies it by
        // care_time_scale for the doze. The shell must do the same or a 6x
        // clock dozes before its settling sounds can reach their marks.
        XCTAssertEqual(sleepCue(idleS: 100 * 6, cooed: false, chirped: false), "coo")
    }

    func testBothMarksArriveBeforeTheDoze() {
        XCTAssertLessThan(sleepCueCooSeconds, sleepCueChirpSeconds)
        XCTAssertLessThan(sleepCueChirpSeconds, 720)
    }
}

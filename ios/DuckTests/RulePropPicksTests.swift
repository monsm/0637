import XCTest
@testable import Duck

/// The block editor's prop group: five one-tap actions built only from words
/// the core already knows. A prop pick may not introduce a trigger or an
/// action the program cannot run.
final class RulePropPicksTests: XCTestCase {
    func testEveryPickUsesAnExistingActionWord() {
        XCTAssertEqual(DuckRulePropPick.all.count, 5)
        let actions = ["skill", "emotion", "sound", "light"]
        for pick in DuckRulePropPick.all {
            XCTAssertTrue(actions.contains(pick.action), "\(pick.id): \(pick.action)")
            XCTAssertFalse(pick.value.isEmpty, "\(pick.id): empty value")
        }
    }

    func testTheFivePropsMapToTheRealVerbs() {
        let byID = Dictionary(uniqueKeysWithValues: DuckRulePropPick.all.map { ($0.id, $0) })
        XCTAssertEqual(byID["ball"]?.action, "skill")
        XCTAssertEqual(byID["ball"]?.value, "kick_left")
        XCTAssertEqual(byID["hoop"]?.action, "skill")
        XCTAssertEqual(byID["hoop"]?.value, "roulade")
        XCTAssertEqual(byID["block"]?.action, "skill")
        XCTAssertEqual(byID["block"]?.value, "ground_pick")
        XCTAssertEqual(byID["cushion"]?.action, "skill")
        XCTAssertEqual(byID["cushion"]?.value, "sit")
        XCTAssertEqual(byID["mirror"]?.action, "emotion")
        XCTAssertEqual(byID["mirror"]?.value, "curious")
    }

    func testIDsAreUniqueAndComplete() {
        XCTAssertEqual(
            Set(DuckRulePropPick.all.map { $0.id }),
            ["ball", "hoop", "block", "cushion", "mirror"]
        )
    }
}

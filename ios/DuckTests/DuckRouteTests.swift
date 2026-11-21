import XCTest
@testable import Duck

/// The iOS reducer obeys the same five rules as Android's JVM-tested
/// `ShellOverlayReducer` (B7). The Android side is the one that runs in
/// minutes on the blind-change machines; these keep the two phones from
/// drifting apart.
final class DuckRouteTests: XCTestCase {
    private func open(_ state: ShellRouteState, _ route: DuckRoute) -> ShellRouteState {
        ShellOverlayReducer.reduce(state, .open(route))
    }

    private func back(_ state: ShellRouteState) -> ShellRouteState {
        ShellOverlayReducer.reduce(state, .back)
    }

    func testOpeningReplacesInsteadOfStacking() {
        var state = ShellRouteState()
        state = open(state, .more)
        state = open(state, .rules)
        XCTAssertEqual(state.sheet, .rules)
        XCTAssertEqual(state.layerCount, 1)
    }

    func testClosingClearsOnlyTheSheet() {
        let state = ShellRouteState(sheet: .diary, overlay: .ring)
        let closed = ShellOverlayReducer.reduce(state, .closeSheet)
        XCTAssertNil(closed.sheet)
        XCTAssertEqual(closed.overlay, .ring)
    }

    func testRingAndSheetCoexist() {
        var state = open(ShellRouteState(), .instrument)
        state = ShellOverlayReducer.reduce(state, .openRing)
        XCTAssertEqual(state.sheet, .instrument)
        XCTAssertEqual(state.overlay, .ring)
        XCTAssertEqual(state.layerCount, 2)
    }

    func testBackClosesRingThenSheetThenStops() {
        var state = ShellRouteState(sheet: .peers, overlay: .ring)
        state = back(state)
        XCTAssertEqual(state.overlay, .none)
        XCTAssertEqual(state.sheet, .peers)
        state = back(state)
        XCTAssertNil(state.sheet)
        XCTAssertEqual(back(state), state)
    }

    func testEveryRouteTagCarriesThePrefixAndIdsAreUnique() {
        let ids = DuckRoute.allCases.map(\.id)
        XCTAssertEqual(ids.count, Set(ids).count)
        for route in DuckRoute.allCases {
            XCTAssertTrue(route.tag.hasPrefix("route-"))
        }
    }
}

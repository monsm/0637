import XCTest
@testable import Duck

/// The walk slot outlives a launch (comparison 2026-11-18 #4). The core's
/// override is process memory, so the shell is its store: these tests cover
/// the stored string and the exact value a replay would hand back. The core
/// round trip needs the dylib and belongs to the simulator's CI run.
final class WalkPolicyTests: XCTestCase {

    /// A store of its own, so a test never reads or writes the device's
    /// real preferences (and no test sees another's leftovers).
    private func freshDefaults() -> UserDefaults {
        let suite = "duck-walk-policy-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suite)
        }
        return defaults
    }

    func testStoredNameSurvivesANewStore() {
        let defaults = freshDefaults()
        WalkPolicyStore(defaults: defaults).remember("chosen_walk.onnx")
        XCTAssertEqual(
            WalkPolicyStore(defaults: defaults).name,
            "chosen_walk.onnx",
            "a walk-slot name must survive the store object that wrote it"
        )
    }

    func testReplayHandsOverExactlyTheStoredString() {
        let defaults = freshDefaults()
        let store = WalkPolicyStore(defaults: defaults)
        store.remember("chosen_walk.onnx")
        var replayed: [String] = []
        let accepted = store.replay { name in
            replayed.append(name)
            return true
        }
        XCTAssertTrue(accepted)
        XCTAssertEqual(replayed, ["chosen_walk.onnx"])
    }

    func testNothingStoredMeansNoRequest() {
        let store = WalkPolicyStore(defaults: freshDefaults())
        var replayed: [String] = []
        let accepted = store.replay { name in
            replayed.append(name)
            return true
        }
        XCTAssertFalse(accepted, "a fresh install has no walk slot to replay")
        XCTAssertTrue(replayed.isEmpty, "a fresh install must not hand the core a name")
    }

    func testNoneIsReplayedVerbatim() {
        let defaults = freshDefaults()
        let store = WalkPolicyStore(defaults: defaults)
        store.remember("none")
        var replayed: [String] = []
        _ = store.replay { name in
            replayed.append(name)
            // The core may refuse this name; it is still the name the shell
            // must hand over, not one the shell silently rewrites.
            return false
        }
        XCTAssertEqual(
            replayed,
            ["none"],
            "the shell replays the stored value verbatim; the core rules on it"
        )
    }

    func testNoneSpellingIsNotRewritten() {
        let defaults = freshDefaults()
        let store = WalkPolicyStore(defaults: defaults)
        store.remember("NONE")
        var replayed: [String] = []
        _ = store.replay { name in
            replayed.append(name)
            return false
        }
        XCTAssertEqual(replayed, ["NONE"], "the store does not normalise the sentinel")
    }

    func testBlankForgetsTheOverride() {
        let defaults = freshDefaults()
        let store = WalkPolicyStore(defaults: defaults)
        store.remember("chosen_walk.onnx")
        store.remember("   ")
        XCTAssertNil(store.name, "a blank name is not a walk file")
        XCTAssertFalse(
            store.replay { _ in true },
            "a blank name leaves nothing to replay"
        )
    }

    func testForgetLeavesNothingToReplay() {
        let defaults = freshDefaults()
        let store = WalkPolicyStore(defaults: defaults)
        store.remember("chosen_walk.onnx")
        store.forget()
        XCTAssertNil(store.name)
        XCTAssertFalse(store.replay { _ in true })
    }
}

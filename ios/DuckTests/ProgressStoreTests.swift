@testable import Duck
import XCTest

/// The achievement chain without a screen (B4): a neighbour gift credits
/// the wall only when the core said ok, and only that event unlocks
/// first_gift. The controller passes the core's own verdict through, so a
/// refused, busy or throttled delivery (the tap happened, the gift did
/// not) never becomes an achievement — that would be inventing a fact.
final class ProgressStoreTests: XCTestCase {

    private func freshStore() -> ProgressStore {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("progress-tests-\(UUID().uuidString)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock {
            try? FileManager.default.removeItem(at: dir)
        }
        return ProgressStore(directory: dir)
    }

    /// record() hops a serial queue; pump the run loop until the published
    /// snapshot catches up or a short deadline passes.
    private func settle(_ store: ProgressStore, id: String, expect: Bool) -> Bool {
        let deadline = Date().addingTimeInterval(5)
        while Date() < deadline {
            if store.unlockedIDs.contains(id) == expect { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        return store.unlockedIDs.contains(id) == expect
    }

    func testCoreAcceptedGiftUnlocksFirstGift() {
        let store = freshStore()
        XCTAssertFalse(
            store.unlockedIDs.contains("first_gift"),
            "a fresh store must start with a grey wall"
        )
        store.recordPeerGift(ok: true)
        XCTAssertTrue(
            settle(store, id: "first_gift", expect: true),
            "a core-accepted gift must unlock first_gift"
        )
        XCTAssertNotNil(store.unlockDates["first_gift"])
    }

    func testRefusedOrThrottledGiftNeverCounts() {
        let store = freshStore()
        store.recordPeerGift(ok: false)
        XCTAssertFalse(
            settle(store, id: "first_gift", expect: false),
            "the core refused this delivery; the wall must stay grey"
        )
    }

    func testOtherEventsDoNotReachTheGiftAchievement() {
        let store = freshStore()
        store.record(.pet)
        store.record(.feed)
        XCTAssertTrue(
            settle(store, id: "first_gift", expect: false),
            "petting and feeding are real facts, but not a neighbour's gift"
        )
    }
}

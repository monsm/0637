import XCTest
@testable import Duck

/// Audit drift #8: the sender used to write the stick into the wrong axes,
/// so a duck driven from a neighbour's phone spun in place instead of
/// walking. The wire order is the official one — lx = left_x at offset 14,
/// ly = left_y at offset 16 — and the parity this file pins is
/// receiver-vs-shell: the core's pad receiver drives
/// `vx = deadzone(ly)*0.4, wz = -deadzone(lx)*1.0`
/// (core/duck-mobile-core/src/ffi.rs), while this shell's own tick drives
/// `vx = -stickY*0.4, wz = -stickX*1.0` (DuckController.tick). Equal
/// sticks must therefore write lx = stickX and ly = -stickY.
final class PadSenderAxisTests: XCTestCase {
    func testLxCarriesTheStickX() {
        XCTAssertEqual(PadSender.axes(stickX: 1, stickY: 0).lx, 32767)
        XCTAssertEqual(PadSender.axes(stickX: -1, stickY: 0).lx, -32767)
        XCTAssertEqual(PadSender.axes(stickX: 0, stickY: 0).lx, 0)
    }

    func testLyCarriesTheNegatedStickY() {
        XCTAssertEqual(PadSender.axes(stickX: 0, stickY: -1).ly, 32767)
        XCTAssertEqual(PadSender.axes(stickX: 0, stickY: 1).ly, -32767)
        XCTAssertEqual(PadSender.axes(stickX: 0, stickY: 0).ly, 0)
    }

    func testTheStickClampsToTheAxisWidth() {
        XCTAssertEqual(PadSender.axes(stickX: 4, stickY: -9).lx, 32767)
        XCTAssertEqual(PadSender.axes(stickX: 4, stickY: -9).ly, 32767)
        XCTAssertEqual(PadSender.axes(stickX: -4, stickY: 9).ly, -32767)
    }

    /// The property the audit demanded: whatever the thumb does, the
    /// remote duck computes the same vx/wz this shell's own tick would
    /// have. The receiver normalises ±32767 back to ±1.0 before its
    /// deadzone and gains; the shell's deadzone sits on the same raw
    /// stick, so it cancels on both sides and the raw parity decides.
    func testReceiverMappingMatchesTheShellOwnTwist() {
        let sticks: [(Double, Double)] = [
            (0, 0), (1, 0), (-1, 0), (0, 1), (0, -1),
            (0.6, -0.8), (-0.35, 0.25),
        ]
        for (x, y) in sticks {
            let axes = PadSender.axes(stickX: x, stickY: y)
            let remoteVx = Double(axes.ly) / 32767.0 * 0.4
            let remoteWz = -Double(axes.lx) / 32767.0 * 1.0
            XCTAssertEqual(remoteVx, -y * 0.4, accuracy: 0.0001)
            XCTAssertEqual(remoteWz, -x * 1.0, accuracy: 0.0001)
        }
    }
}

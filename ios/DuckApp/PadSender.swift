import Darwin
import Foundation

/// This phone as the official DKPD pad — the iOS half of the protocol in
/// `upstream/microduck/docs/robot/udp-pad.md`.
///
/// 28 bytes, little-endian, thirty times a second, exactly the packet a
/// laptop or a pad sends the robot: magic "DKPD", version 1, a wrapping
/// seq, the sender's own clock, six axes and a button word. The receiving
/// half is the core's `pad.rs` on the other phone, still gated by the pair
/// code — this sender only dials paired neighbours.
///
/// Raw BSD sockets, not URLSession: this is a datagram at 30 Hz, and the
/// shell's joystick is the only input it reads.
final class PadSender {
    private var socket: Int32 = -1
    private var target = sockaddr_in()
    private var thread: Thread?
    private var running = false
    private var seq: UInt32 = 0

    /// The stick, read every frame; the shell owns it.
    var stickX = 0.0
    var stickY = 0.0

    /// Dial one paired neighbour; false for a junk address.
    func start(addr: String) -> Bool {
        let host = addr.components(separatedBy: ":").first ?? addr
        var address = sockaddr_in()
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = in_port_t(4210).bigEndian
        guard inet_pton(AF_INET, host, &address.sin_addr) == 1 else { return false }
        stop()
        socket = Darwin.socket(AF_INET, SOCK_DGRAM, 0)
        guard socket >= 0 else { return false }
        target = address
        running = true
        let thread = Thread { [weak self] in
            guard let self else { return }
            var buffer = [UInt8](repeating: 0, count: 28)
            while self.running {
                self.seq &+= 1
                buffer[0] = UInt8(ascii: "D"); buffer[1] = UInt8(ascii: "K")
                buffer[2] = UInt8(ascii: "P"); buffer[3] = UInt8(ascii: "D")
                buffer[4] = 1 // version
                buffer[5] = 0 // flags
                Self.putU32(&buffer, 6, self.seq)
                Self.putU32(&buffer, 10, UInt32(truncatingIfNeeded: Int(Date().timeIntervalSince1970 * 1000)))
                // lx is left (+yaw), ly is forward (+vx); raw values, the
                // receiver owns the deadzone.
                Self.putI16(&buffer, 14, Int16((max(-1.0, min(1.0, -self.stickY)) * 32767).rounded()))
                Self.putI16(&buffer, 16, Int16((max(-1.0, min(1.0, self.stickX)) * 32767).rounded()))
                // rx, ry, lt, rt and the button word stay zero: the sender
                // keeps the verbs the receiving shell's own controls own.
                var destination = self.target
                buffer.withUnsafeBytes { raw in
                    withUnsafePointer(to: &destination) { pointer in
                        pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
                            _ = sendto(
                                self.socket,
                                raw.baseAddress,
                                raw.count,
                                0,
                                sockaddrPointer,
                                socklen_t(MemoryLayout<sockaddr_in>.size)
                            )
                        }
                    }
                }
                Thread.sleep(forTimeInterval: 0.033)
            }
        }
        thread.name = "duck-pad-sender"
        self.thread = thread
        thread.start()
        return true
    }

    func stop() {
        running = false
        thread?.cancel()
        thread = nil
        if socket >= 0 {
            Darwin.close(socket)
            socket = -1
        }
    }

    var active: Bool { running }

    private static func putU32(_ buffer: inout [UInt8], _ at: Int, _ value: UInt32) {
        let little = value.littleEndian
        buffer[at] = UInt8(truncatingIfNeeded: little)
        buffer[at + 1] = UInt8(truncatingIfNeeded: little >> 8)
        buffer[at + 2] = UInt8(truncatingIfNeeded: little >> 16)
        buffer[at + 3] = UInt8(truncatingIfNeeded: little >> 24)
    }

    private static func putI16(_ buffer: inout [UInt8], _ at: Int, _ value: Int16) {
        let little = UInt16(bitPattern: value).littleEndian
        buffer[at] = UInt8(truncatingIfNeeded: little)
        buffer[at + 1] = UInt8(truncatingIfNeeded: little >> 8)
    }
}
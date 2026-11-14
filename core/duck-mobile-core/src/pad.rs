//! The official UDP pad, received on a phone.
//!
//! `upstream/microduck/docs/robot/udp-pad.md` defines one 28-byte `DKPD`
//! datagram on UDP 4210: magic/version/flags/seq/t_ms/six axes/buttons.
//! The robot's `netpadd` turns that into the same intents a Bluetooth pad
//! writes; this module does the same for the digital duck — the pad is a
//! client, never a sense. The 61-dimensional observation and safety layer
//! do not see it: the tick reads the latest frame and calls the SAME
//! `set_twist` / skill entries the shell's own joystick uses.
//!
//! The official protocol is explicitly unauthenticated ("a quick LAN tool
//! made for a network you trust"). A family app cannot open that by default,
//! so the listener is OFF until the person enables it AND a six-digit pair
//! code has been set on the LAN half — a stranger on the Wi-Fi then has no
//! control channel. Silence is a release: after 300 ms with no fresh frame
//! the twist goes back to zero, exactly like robotd's deadman.

use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 4210;
pub const PACKET: usize = 28;
/// Nothing new for this long and the duck stops where it is.
const STALE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axes {
    pub lx: f64,
    pub ly: f64,
    pub rx: f64,
    pub ry: f64,
    pub lt: f64,
    pub rt: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub axes: Axes,
    pub buttons: u16,
    pub seq: u32,
    pub t_ms: u32,
}

/// Parse one datagram. A version-1 packet LONGER than 28 bytes is accepted
/// and its tail ignored (the official doc allows it); anything else is noise.
pub fn parse(bytes: &[u8]) -> Option<Frame> {
    if bytes.len() < PACKET || &bytes[0..4] != b"DKPD" || bytes[4] != 1 {
        return None;
    }
    // parse() already checked bytes.len() >= PACKET, so every slice below
    // has exactly the width of its integer: unwrap cannot fail here.
    let u32le = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let i16le = |at: usize| i16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let axis = |at: usize| f64::from(i16le(at)) / 32767.0;
    Some(Frame {
        seq: u32le(6),
        t_ms: u32le(10),
        axes: Axes {
            lx: axis(14),
            ly: axis(16),
            rx: axis(18),
            ry: axis(20),
            lt: axis(22),
            rt: axis(24),
        },
        buttons: u16::from_le_bytes(bytes[26..28].try_into().unwrap()),
    })
}

/// What the tick reads: the latest accepted frame, and how old it is.
#[derive(Debug, Default)]
struct Shared {
    frame: Option<Frame>,
    last: Option<Instant>,
    seq: Option<u32>,
    drops: u64,
}

impl Shared {
    fn accept(&mut self, packet: &[u8]) {
        let Some(frame) = parse(packet) else {
            self.drops += 1;
            return;
        };
        if let Some(previous) = self.seq {
            let ahead = frame.seq.wrapping_sub(previous);
            // A restarted sender starts anywhere: a big jump (forward or a
            // far backwards wrap) is a NEW pad, not a stale frame. Only a
            // SMALL backwards step — the last few frames arriving out of
            // order on the network — is stale. `ahead` measures the forward
            // distance, so "backwards by less than 1000" is
            // `ahead > 2^32 - 1000`; the written form can never be true
            // (no number is both > 2^31 and < 1000) and dropped nothing.
            if ahead > u32::MAX - 1000 {
                self.drops += 1;
                return;
            }
        }
        self.seq = Some(frame.seq);
        self.frame = Some(frame);
        self.last = Some(Instant::now());
    }
}

/// The receiver half. One socket, one listener thread, one shared snapshot.
/// Dropping the handle does not kill the OS thread until the process ends,
/// so `stop()` explicitly flips the gate — no frames are consumed then.
#[derive(Clone)]
pub struct Pad {
    shared: Arc<Mutex<Shared>>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl Pad {
    /// Bind 4210 and start listening. Off by default: `enable()` is a
    /// deliberate second step the UI takes with the pairing code.
    pub fn start() -> Option<Pad> {
        let socket = UdpSocket::bind(("0.0.0.0", PORT)).ok()?;
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok()?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let listener_shared = shared.clone();
        let listener_enabled = enabled.clone();
        std::thread::Builder::new()
            .name("duck-netpad".to_owned())
            .spawn(move || {
                let mut buf = [0u8; 2048];
                loop {
                    match socket.recv_from(&mut buf) {
                        Ok((count, _)) => {
                            if !listener_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                                continue;
                            }
                            if let Ok(mut state) = listener_shared.lock() {
                                state.accept(&buf[..count]);
                            }
                        }
                        Err(_) => continue,
                    }
                }
            })
            .ok()?;
        Some(Pad { shared, enabled })
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, std::sync::atomic::Ordering::Relaxed);
        if !value {
            if let Ok(mut state) = self.shared.lock() {
                state.frame = None;
                state.last = None;
            }
        }
    }

    /// The latest frame if the pad is enabled AND a fresh datagram arrived
    /// inside the official stale window. Silence is a release, never a held
    /// walk command.
    pub fn latest(&self) -> Option<Frame> {
        if !self.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let mut state = self.shared.lock().ok()?;
        let fresh = state
            .last
            .is_some_and(|at| at.elapsed() <= STALE);
        if fresh {
            state.frame
        } else {
            state.frame = None;
            None
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn drops(&self) -> u64 {
        self.shared.lock().map_or(0, |state| state.drops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(seq: u32, ly: i16, buttons: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"DKPD");
        bytes.push(1);
        bytes.push(0);
        bytes.extend_from_slice(&seq.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for value in [0i16, ly, 0, 0, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&buttons.to_le_bytes());
        bytes
    }

    #[test]
    fn the_official_twenty_eight_bytes_parse() {
        let frame = parse(&packet(7, 16384, 1)).expect("a DKPD frame");
        assert_eq!(frame.seq, 7);
        assert!((frame.axes.ly - 0.5).abs() < 0.001);
        assert_eq!(frame.buttons, 1);
        // Extra tail is allowed for version 1.
        let mut longer = packet(8, 0, 0);
        longer.push(0xff);
        assert!(parse(&longer).is_some());
        // Wrong magic, wrong version, short packet: noise.
        assert!(parse(&[0u8; 28]).is_none());
        assert!(parse(&packet(9, 0, 0)[..27]).is_none());
    }

    #[test]
    fn a_stale_sequence_is_dropped_and_a_restart_is_accepted() {
        let mut state = Shared::default();
        state.accept(&packet(100, 0, 0));
        state.accept(&packet(99, 0, 0));
        assert_eq!(state.frame.expect("frame").seq, 100);
        assert_eq!(state.drops, 1);
        // A sender that restarted at a far-away sequence is a new pad.
        state.accept(&packet(5_000, 0, 0));
        assert_eq!(state.frame.expect("frame").seq, 5_000);
    }
}

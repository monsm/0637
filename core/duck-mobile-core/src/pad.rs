//! The official UDP pad, received on a phone — all three datagrams of it.
//!
//! `upstream/microduck/docs/robot/udp-pad.md` defines the protocol: one 28-byte `DKPD`
//! datagram on UDP 4210 carries the pad's whole state; `DKST` (24 bytes) is the answer
//! that tells a sender which datagram was taken and the receiver's running counts
//! (`pad-map/src/wire.rs:162-252`); `DKRB` carries the robot's health, posture and last
//! refusal to the driving client once a second (`pad-map/src/wire.rs:254-344`). The
//! robot's `netpadd` turns DKPD into the same intents a Bluetooth pad writes; this
//! module does the same for the digital duck — the pad is a client, never a sense. The
//! 61-dimensional observation and safety layer do not see it: the tick reads the latest
//! frame and calls the SAME `set_twist` / skill entries the shell's own joystick uses.
//!
//! The official protocol is explicitly unauthenticated ("a quick LAN tool made for a
//! network you trust"). A family app cannot open that by default, so the listener is
//! OFF until the person enables it AND a six-digit pair code has been set on the LAN
//! half — a stranger on the Wi-Fi then has no control channel. Silence is a release:
//! after 300 ms with no fresh frame the twist goes back to zero, exactly like robotd's
//! deadman; after 1000 ms the client counts as gone and the lock is released
//! (`robotd-params/src/lib.rs` `NetpadParams` defaults: stale_ms 300, timeout_ms 1000;
//! `docs/robot/udp-pad.md` table lines 72-73).
//!
//! # Provenance of the official semantics
//!
//! Two upstream files are compiled **verbatim** with `#[path]` (the same pattern
//! `control.rs` uses for `robotd/src/control.rs`):
//!
//! - `pad-map/src/buttons.rs` — the bit table. The bit order IS the wire's, so a
//!   datagram's `buttons` field is a [`Buttons`] without translation.
//! - `pad-map/src/hold.rs` — the Start/Select hold state machine, thresholds included,
//!   with its dropout (pad-gone) semantics.
//!
//! Both depend on `std` only, so they compile here unchanged.
//!
//! `wire.rs` and `mapper.rs` cannot be `#[path]`-included: they address
//! `crate::{Buttons, PadFrame}` — paths into pad-map's own crate root — and
//! `mapper.rs` also calls `pad_imu`, a crate this build does not otherwise carry.
//! Everything they define is therefore ported **line for line** below, each item
//! commented with its upstream file and line range, and byte layout is asserted
//! against the official `encode()` output in tests. The binding table is NOT a copy
//! at all: it is the official `robotd_params::PadParams` type itself, so the
//! defaults (A=sit_toggle, B=ground_pick, Y=excited, LB=yes, RB=no —
//! `robotd-params/src/lib.rs:416-427`) cannot drift.

// Verbatim official sources. `#[path]` is relative to this file's directory
// (`src/`), so three levels up is the workspace root (`D:\AI\Duck`).
#[path = "../../../upstream/microduck/pad-map/src/buttons.rs"]
pub mod buttons;
#[path = "../../../upstream/microduck/pad-map/src/hold.rs"]
pub mod hold;

pub use buttons::Buttons;
pub use hold::{HOME_HOLD, HoldAction, HoldButton, REST_HOLD, SHUTDOWN_HOLD};

use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 4210;
pub const PACKET: usize = 28;
/// Nothing new for this long and the duck stops where it is.
/// `[netpad] stale_ms` default (`robotd-params/src/lib.rs:341`).
pub const STALE_MS: u64 = 300;
const STALE: Duration = Duration::from_millis(STALE_MS);
/// Silence after which the client counts as gone and the lock is released.
/// `[netpad] timeout_ms` default (`robotd-params/src/lib.rs:342`).
pub const TIMEOUT_MS: u64 = 1000;
/// How far ahead of the last accepted `seq` a datagram may be and still count as
/// newer (`netpadd/src/receiver.rs:22`). A forged datagram far ahead would
/// otherwise read every real one as stale.
pub const MAX_SEQ_AHEAD: u32 = 1000;
/// `DKST` answer length (`pad-map/src/wire.rs:165`) — no longer than a pad
/// datagram, so an answer never amplifies what it answers.
pub const STATUS_LEN: usize = 24;
/// How often a client that does not hold the robot is told so, at most
/// (`pad-map/src/wire.rs:173`).
pub const BUSY_EVERY: Duration = Duration::from_secs(1);
/// How often the driving client is sent a [`RobotReport`]
/// (`pad-map/src/wire.rs:257`).
pub const REPORT_EVERY: Duration = Duration::from_secs(1);
/// A refusal is reported for this long after it happened, then dropped
/// (`pad-map/src/wire.rs:259`).
pub const REFUSAL_KEPT: Duration = Duration::from_secs(30);
/// How far a trigger goes down before it sounds (`pad-map/src/mapper.rs:106`).
pub const SOUND_THRESHOLD: f64 = 0.3;
/// `flags` bit 0: the sender plays the triggers' sounds itself, so the robot keeps
/// quiet (`pad-map/src/wire.rs:30`).
pub const SENDER_SOUNDS: u8 = 1 << 0;

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
    /// Bit 0 of `flags`: the sender plays the trigger sounds itself
    /// (`pad-map/src/wire.rs:30,105-107`).
    pub sender_sounds: bool,
}

/// Why a datagram is not pad state (`pad-map/src/wire.rs:48-52`, `WireError`):
/// too short, wrong magic, or a version this build does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    Short(usize),
    Magic,
    Version(u8),
}

/// The 28-byte (or longer) version-1 datagram, decoded — `pad-map/src/wire.rs:80-102`
/// (`Packet::decode`), ported verbatim in structure. A version-1 packet LONGER than
/// 28 bytes is accepted and its tail ignored (appended fields a v1 reader does not
/// know); anything else is noise. Returned as this module's [`Frame`] (raw axes
/// plus the derived `sender_sounds`); axis scaling happens in [`Frame::pad_frame`].
pub fn parse(bytes: &[u8]) -> Option<Frame> {
    match decode_packet(bytes) {
        Ok(frame) => Some(frame),
        Err(_) => None,
    }
}

/// `pad-map/src/wire.rs:80-102`, verbatim in structure: length, magic, version,
/// then the little-endian fields at the documented offsets.
fn decode_packet(b: &[u8]) -> Result<Frame, WireError> {
    if b.len() < PACKET {
        return Err(WireError::Short(b.len()));
    }
    if b[0..4] != *b"DKPD" {
        return Err(WireError::Magic);
    }
    if b[4] != 1 {
        return Err(WireError::Version(b[4]));
    }
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let i16_at = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]);
    Ok(Frame {
        seq: u32_at(6),
        t_ms: u32_at(10),
        axes: Axes {
            lx: f64::from(i16_at(14)),
            ly: f64::from(i16_at(16)),
            rx: f64::from(i16_at(18)),
            ry: f64::from(i16_at(20)),
            lt: f64::from(i16_at(22)),
            rt: f64::from(i16_at(24)),
        },
        buttons: u16::from_le_bytes([b[26], b[27]]),
        sender_sounds: b[5] & SENDER_SOUNDS != 0,
    })
}

impl Frame {
    /// The raw `buttons` field masked to the known bits (`pad-map/src/wire.rs:109-111`).
    pub fn held(&self) -> Buttons {
        Buttons(self.buttons & Buttons::KNOWN)
    }

    /// Button bits this build does not know (`pad-map/src/wire.rs:113-115`); logged
    /// once by the receiver, never acted on.
    pub fn reserved(&self) -> u16 {
        self.buttons & !Buttons::KNOWN
    }

    /// `pad-map/src/wire.rs:117-134` (`Packet::frame`): the raw i16 axes scaled the
    /// official way — sticks ±32767 to ±1 clamped (up positive), triggers 0..32767
    /// to 0..1 clamped (never negative).
    pub fn pad_frame(&self, pressed: Buttons, released: Buttons) -> PadFrame {
        let stick = |v: f64| (v / 32767.0).clamp(-1.0, 1.0);
        let trigger = |v: f64| (v / 32767.0).clamp(0.0, 1.0);
        PadFrame {
            left_x: stick(self.axes.lx),
            left_y: stick(self.axes.ly),
            right_x: stick(self.axes.rx),
            right_y: stick(self.axes.ry),
            lt: trigger(self.axes.lt),
            rt: trigger(self.axes.rt),
            held: self.held(),
            pressed,
            released,
        }
    }
}

/// One instant of a pad (`pad-map/src/mapper.rs:85-101`), without the fields that
/// only a Bluetooth pad or a pad IMU can set (`attitude`, `has_imu` — always
/// `None`/false from a UDP sender, `mapper.rs:97-100`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadFrame {
    pub left_x: f64,
    pub left_y: f64,
    pub right_x: f64,
    pub right_y: f64,
    pub lt: f64,
    pub rt: f64,
    pub held: Buttons,
    pub pressed: Buttons,
    pub released: Buttons,
}

/// What became of the datagram a [`Status`] answers (`pad-map/src/wire.rs:177-202`).
/// `Driving` — from the client that holds the robot, taken or dropped as stale.
/// `Busy` — another client holds the robot. `Other(c)` — a code this build does not
/// know, kept so a reader can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Driving,
    Busy,
    Other(u8),
}

impl Verdict {
    fn code(self) -> u8 {
        match self {
            Verdict::Driving => 1,
            Verdict::Busy => 2,
            Verdict::Other(c) => c,
        }
    }

    fn from_code(c: u8) -> Self {
        match c {
            1 => Verdict::Driving,
            2 => Verdict::Busy,
            c => Verdict::Other(c),
        }
    }
}

/// `netpadd`'s answer to one pad datagram (`pad-map/src/wire.rs:207-251`, layout at
/// `docs/robot/udp-pad.md:272-282`): 24 bytes, magic `DKST`, version 1, verdict
/// code, the seq answered, then the receiver's own wrapping counters — accepted
/// datagrams taken from the driving client, stale ones dropped from it, and
/// malformed ones from anyone. A sender reads their change, never their value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub verdict: Verdict,
    pub seq: u32,
    pub accepted: u32,
    pub stale: u32,
    pub malformed: u32,
}

impl Status {
    pub fn encode(&self) -> [u8; STATUS_LEN] {
        let mut b = [0u8; STATUS_LEN];
        b[..4].copy_from_slice(b"DKST");
        b[4] = 1;
        b[5] = self.verdict.code();
        b[6..10].copy_from_slice(&self.seq.to_le_bytes());
        b[10..14].copy_from_slice(&self.accepted.to_le_bytes());
        b[14..18].copy_from_slice(&self.stale.to_le_bytes());
        b[18..22].copy_from_slice(&self.malformed.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Result<Self, WireError> {
        if b.len() < STATUS_LEN {
            return Err(WireError::Short(b.len()));
        }
        if b[0..4] != *b"DKST" {
            return Err(WireError::Magic);
        }
        if b[4] != 1 {
            return Err(WireError::Version(b[4]));
        }
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(Status {
            verdict: Verdict::from_code(b[5]),
            seq: u32_at(6),
            accepted: u32_at(10),
            stale: u32_at(14),
            malformed: u32_at(18),
        })
    }
}

/// What the robot is doing (`pad-map/src/wire.rs:287-295`): from `robot.state`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Posture {
    /// The policy that drove the last tick: `walk`, `stand`, `held`, …
    pub policy: String,
    pub fallen: bool,
    pub limp: bool,
    pub picked_up: bool,
}

/// A request from the pad the robot did not carry out, and its reason
/// (`pad-map/src/wire.rs:298-306`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Refusal {
    /// The method, `robot.do`, `robot.enable`, …
    pub method: String,
    pub reason: String,
    /// How long before this report it happened.
    pub ago_ms: u64,
}

/// What the robot is, as the driving client is told it
/// (`pad-map/src/wire.rs:267-284`): magic `DKRB`, version byte 1, then a JSON
/// object. JSON rather than a fixed layout because it carries `robot.health`
/// unchanged — a field added there reaches a client without a second format to
/// keep in step. Every field is optional: an absent one is *not known*, never
/// zero. `health` is the official `duck_ipc_proto::HealthResult` itself, so its
/// JSON field names are the struct's own.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct RobotReport {
    /// `robot.health`, as the robot last answered it
    /// (`duck-ipc-proto/src/lib.rs:3946`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<duck_ipc_proto::HealthResult>,
    /// Why there is no `health`: the robot not answering, or not answering in time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posture: Option<Posture>,
    /// Why there is no `posture`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posture_error: Option<String>,
    /// The last request the robot said no to, within [`REFUSAL_KEPT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refusal>,
}

impl RobotReport {
    /// `pad-map/src/wire.rs:324-330`. A struct of options, strings and numbers:
    /// serialising it cannot fail.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = b"DKRB".to_vec();
        b.push(1);
        b.extend(serde_json::to_vec(self).unwrap_or_default());
        b
    }

    pub fn decode(b: &[u8]) -> Result<Self, WireError> {
        if b.len() < 5 {
            return Err(WireError::Short(b.len()));
        }
        if b[0..4] != *b"DKRB" {
            return Err(WireError::Magic);
        }
        if b[4] != 1 {
            return Err(WireError::Version(b[4]));
        }
        serde_json::from_slice(&b[5..]).map_err(|_| WireError::Magic)
    }
}

/// What a report is built from: the caller injects the robot's own facts, this
/// module only frames them. Fields are options so "not known" stays distinct from
/// zero, exactly as the wire defines it.
#[derive(Debug, Clone, Default)]
pub struct PadReportInput {
    pub health: Option<duck_ipc_proto::HealthResult>,
    pub health_error: Option<String>,
    pub posture: Option<Posture>,
    pub posture_error: Option<String>,
}

/// The refusal memory (`netpadd/src/main.rs:190,366-368,493-510`): the last pad
/// request the robot said no to, kept [`REFUSAL_KEPT`] (30 s) and reported with
/// `ago_ms` counting from when it happened. `remember_refused` overwrites — the
/// wire carries only the LAST refusal.
#[derive(Debug, Default)]
pub struct Refused {
    at: Option<(String, String, Instant)>,
}

impl Refused {
    /// Record that a request named `method` was refused for `reason`, now.
    pub fn remember_refused(&mut self, method: &str, reason: &str, now: Instant) {
        self.at = Some((method.to_owned(), reason.to_owned(), now));
    }

    /// The refusal to put in the current report, if one is still inside the 30 s
    /// window (`netpadd/src/main.rs:503-509`, verbatim arithmetic: filter on
    /// `REFUSAL_KEPT`, `ago_ms` from the elapsed time).
    pub fn refusal(&self, now: Instant) -> Option<Refusal> {
        let (method, reason, at) = self.at.as_ref()?;
        if now.duration_since(*at) >= REFUSAL_KEPT {
            return None;
        }
        Some(Refusal {
            method: method.clone(),
            reason: reason.clone(),
            ago_ms: now.duration_since(*at).as_millis() as u64,
        })
    }
}

/// What the sticks drive, picked on the D-pad (`pad-map/src/mapper.rs:14-34`).
/// Modal because two sticks cannot express nine degrees of freedom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadMode {
    /// D-pad left. Left stick walks and strafes, right stick turns.
    #[default]
    Drive,
    /// D-pad up. All four axes pose the head; the body holds still.
    Head,
    /// D-pad right. Left stick walks and turns, right stick looks around.
    HeadDrive,
    /// D-pad down. Body + head: the left stick crouches and leans the standing
    /// robot sideways, the right stick looks around. The body does not walk.
    BodyPose,
}

impl PadMode {
    /// Whether this mode sends head poses, so leaving it has a head to put back
    /// (`pad-map/src/mapper.rs:31-33`). The mode-exit intents themselves are the
    /// follow-up ticket's wiring; this is the official predicate it will need.
    pub fn poses_head(self) -> bool {
        matches!(self, PadMode::Head | PadMode::HeadDrive | PadMode::BodyPose)
    }
}

/// What a pad button asks the robot to do. The event vocabulary the official
/// mapping produces; the caller maps it onto skills and intents. Every variant is
/// the port of one official intent:
///
/// - `Skill`: one of the six bindable buttons (A/B/X/Y/LB/RB) pressed, carrying
///   its `[pad]` config name (`pad-map/src/mapper.rs:312-328`). The bound skill
///   comes from the table — officially a=sit_toggle, b=ground_pick, x="",
///   y=excited, lb=yes, rb=no (`robotd-params/src/lib.rs:416-427`) — and an empty
///   binding produces nothing.
/// - `Init`: Start tapped while the robot is not up — `robot.init`, stand up and
///   hold (`mapper.rs:238-244`).
/// - `Toggle`: Start tapped while the robot is up — `robot.enable toggle`. The
///   robot owns the state: turning OFF returns it to the home pose, so turning on
///   always starts the policy from home (`mapper.rs:245-259`). No local belief
///   decides sit or stand here; the robot's answer does.
/// - `Flashlight`: Home pressed, once per press (`mapper.rs:330-340`).
/// - `Chirp`: RT crossed [`SOUND_THRESHOLD`] going up (`mapper.rs:404-409`).
/// - `Wheee`/`WheeeOff`: LT entered/left its ride (`mapper.rs:410-420`).
/// - `Home`: Start held [`HOME_HOLD`] — home pose, motors stiff, policy off
///   (`mapper.rs:262-281`).
/// - `Rest`: Select held [`REST_HOLD`] — sit, rest pose, torque off and servo
///   reboot (`mapper.rs:361-378`).
/// - `Shutdown`: Select held [`SHUTDOWN_HOLD`] — power off (`mapper.rs:379-383`).
/// - `ModeChange`: a D-pad arrow switched what the sticks mean
///   (`mapper.rs:283-289`).
#[derive(Debug, Clone, PartialEq)]
pub enum PadButtonEvent {
    Skill(&'static str),
    Init,
    Toggle,
    Flashlight,
    Chirp,
    Wheee,
    WheeeOff,
    Home,
    Rest,
    Shutdown,
    ModeChange(PadMode),
}

/// The mapping's memory between frames (`pad-map/src/mapper.rs:136-153`), plus the
/// binding table, which the wire's read/write accessors go through.
#[derive(Debug)]
pub struct PadMapper {
    mode: PadMode,
    start: HoldButton,
    select: HoldButton,
    /// Trigger levels last tick, for the sound edges (`mapper.rs:146-148`).
    prev_rt: f64,
    prev_lt: f64,
    /// Does this pad think the robot is up (torque on, at the home pose)? When
    /// not, Start sends Init instead of the toggle (`mapper.rs:149-152`).
    /// Starts false: padd starts with the robot, and a robotd restart restarts
    /// padd too.
    up: bool,
    /// Which skill each bindable button runs — the official
    /// [`robotd_params::PadParams`], so the shipped defaults (A=sit_toggle,
    /// B=ground_pick, Y=excited, LB=yes, RB=no, X empty) come from upstream, not
    /// a re-typed table (`robotd-params/src/lib.rs:416-427`).
    pub bindings: robotd_params::PadParams,
}

impl Default for PadMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl PadMapper {
    pub fn new() -> Self {
        Self {
            mode: PadMode::Drive,
            start: HoldButton::default(),
            select: HoldButton::default(),
            prev_rt: 0.0,
            prev_lt: 0.0,
            up: false,
            bindings: robotd_params::PadParams::default(),
        }
    }

    /// The pad went away (`pad-map/src/mapper.rs:176-180`): a hold in flight was
    /// measured against it — see `HoldButton::reset` (`hold.rs:97-100`).
    pub fn pad_gone(&mut self) {
        self.start.reset();
        self.select.reset();
    }

    /// What the sticks currently mean.
    pub fn mode(&self) -> PadMode {
        self.mode
    }

    /// The pad's belief about the robot being up, for callers that want to read
    /// it (`mapper.rs` keeps it private and acts on it directly).
    pub fn up(&self) -> bool {
        self.up
    }

    /// Feed the robot's own answer back: `mapper.rs` sets `up` from what the
    /// robot said (`mapper.rs:243,280,376,380`). Call this when a toggle/init
    /// answer lands, so the next Start tap does the right thing.
    pub fn set_up(&mut self, up: bool) {
        self.up = up;
    }

    /// Rebind one of the six bindable buttons by its `[pad]` config name
    /// (`robotd-params/src/lib.rs:447-459`). `false` for a button this build does
    /// not have. An empty `skill` is a button switched off on purpose.
    pub fn bind(&mut self, button: &str, skill: &str) -> bool {
        self.bindings.bind(button, skill)
    }

    /// What a bindable button runs (`robotd-params/src/lib.rs:434-444`). `None`
    /// for a name this build has no button for.
    pub fn skill(&self, button: &str) -> Option<&str> {
        self.bindings.skill(button)
    }

    /// One frame of pad in, the events it means out (`pad-map/src/mapper.rs:184-438`,
    /// ported line for line; the `proto::Call` intents become [`PadButtonEvent`] and
    /// the continuous stick frame becomes [`PadMode`], which the caller turns into
    /// twist/head commands with its own shaping).
    ///
    /// `now` must move forward across calls — the hold thresholds are measured
    /// against it. Returns the events in the order the official mapping asks for
    /// them.
    pub fn tick(&mut self, pad: &PadFrame, now: Instant, events: &mut Vec<PadButtonEvent>) {
        events.clear();

        // The D-pad picks what the sticks mean. Two arrows pressed inside one
        // frame resolve in this fixed order, the last one winning — which is what
        // gilrs's event order did in `padd` for the only case that matters, one
        // arrow at a time (`mapper.rs:206-219`).
        let mut wanted_mode = None;
        for (button, mode) in [
            (Buttons::UP, PadMode::Head),
            (Buttons::RIGHT, PadMode::HeadDrive),
            (Buttons::LEFT, PadMode::Drive),
            (Buttons::DOWN, PadMode::BodyPose),
        ] {
            if pad.pressed.contains(button) {
                wanted_mode = Some(mode);
            }
        }
        // Which bindable buttons went down this frame, by their config name
        // (`mapper.rs:220-225`).
        let pressed = Buttons::BINDABLE
            .iter()
            .filter(|(b, _)| pad.pressed.contains(*b))
            .map(|(_, n)| *n);

        // Start: a tap stands the robot up, then toggles the policy; held, it is
        // the way home (`mapper.rs:227-260`). The robot owns the toggle — a local
        // on/off belief here drifts from the robot's the moment anything else
        // moves it — so the tap is Init while the pad believes the robot is not
        // up, and the toggle event once it does; `set_up` feeds the robot's own
        // answers back.
        let mut go_home = false;
        match self.start.tick(
            pad.held.contains(Buttons::START),
            pad.released.contains(Buttons::START),
            now,
            &[HOME_HOLD],
        ) {
            // Start's one threshold acts when it is reached; its release says
            // nothing more.
            HoldAction::Nothing | HoldAction::ReleasedAfter(_) => {}
            HoldAction::Reached(_) => go_home = true,
            HoldAction::Tap if !self.up => {
                events.push(PadButtonEvent::Init);
                // `padd` set this on the answer and exited on a failed send, so
                // the next frame sees the same state either way.
                self.up = true;
            }
            HoldAction::Tap => {
                events.push(PadButtonEvent::Toggle);
            }
        }

        if go_home {
            // Everything back: the sticks to plain driving, the policy off, and
            // the robot ramped to its home pose with torque on — from limp, from
            // mid-walk or from a crouch alike. The disable comes first so the
            // policy cannot pick the robot up again the moment the ramp ends
            // (`mapper.rs:262-281`).
            self.mode = PadMode::Drive;
            events.push(PadButtonEvent::Home);
            self.up = true;
        }

        if let Some(next) = wanted_mode {
            if next != self.mode {
                self.mode = next;
                events.push(PadButtonEvent::ModeChange(self.mode));
            }
        }

        // One-shot skills (`mapper.rs:312-328`). An empty binding is a button
        // switched off on purpose, not a fault.
        for button in pressed {
            let skill = self.bindings.skill(button).unwrap_or_default();
            if skill.is_empty() {
                continue;
            }
            events.push(PadButtonEvent::Skill(
                // `BINDABLE`'s names are 'static; the slot string is not, so the
                // event carries the BUTTON name and the caller reads the bound
                // skill through `skill()`.
                button,
            ));
        }

        // Home: the flashlight. Not bindable — it is not a skill, and the robot
        // owns the toggle (`mapper.rs:330-340`).
        if pad.pressed.contains(Buttons::HOME) {
            events.push(PadButtonEvent::Flashlight);
        }

        // Select: nothing on a tap. Held two seconds, a rest, at once — the robot
        // sitting down is how the person holding it knows the two seconds are up.
        // Still held at four, a power-off. The release does nothing either way
        // (`mapper.rs:357-388`).
        match self.select.tick(
            pad.held.contains(Buttons::SELECT),
            pad.released.contains(Buttons::SELECT),
            now,
            &[REST_HOLD, SHUTDOWN_HOLD],
        ) {
            HoldAction::Reached(0) => {
                self.up = false;
                events.push(PadButtonEvent::Rest);
            }
            HoldAction::Reached(_) => {
                self.up = false;
                events.push(PadButtonEvent::Shutdown);
            }
            HoldAction::Tap | HoldAction::Nothing | HoldAction::ReleasedAfter(_) => {}
        }

        // Either trigger opens the mouth; the max wins, as in the prototype
        // (`mapper.rs:390-397`) — the mouth is continuous, the caller reads
        // `frame.rt.max(frame.lt)`. Chirp on the right trigger's rising edge; the
        // wheee rides the left trigger and is cut instantly on release
        // (`mapper.rs:399-422`).
        let rt = pad.rt;
        let lt = pad.lt;
        if self.prev_rt < SOUND_THRESHOLD && rt >= SOUND_THRESHOLD {
            events.push(PadButtonEvent::Chirp);
        }
        if lt >= SOUND_THRESHOLD {
            events.push(PadButtonEvent::Wheee);
        } else if self.prev_lt >= SOUND_THRESHOLD {
            events.push(PadButtonEvent::WheeeOff);
        }
        self.prev_rt = rt;
        self.prev_lt = lt;
    }
}

/// What became of one datagram, in the receiver's vocabulary
/// (`netpadd/src/receiver.rs:44-55`). The mobile listener keeps the parts the
/// wire needs — verdict, counters, edges — and leaves the lateness judgement to
/// the keepalive-free world of a phone: a datagram is judged by seq only, the way
/// the 300 ms stale window already covers delivery for the twist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    /// Accepted, and it is the first since the client was last gone.
    Connected,
    Taken,
    /// Not newer than what is held, or further ahead than [`MAX_SEQ_AHEAD`].
    Stale,
    /// Another sender holds the lock.
    OtherPeer,
    Malformed,
}

/// The receiver's memory (`netpadd/src/receiver.rs:70-91`, the seq/edge subset —
/// a phone has no per-stream latency clock to judge, and the stale window is the
/// twist's own).
#[derive(Debug, Default)]
struct Shared {
    frame: Option<Frame>,
    last: Option<Instant>,
    /// The client that holds the duck, while it is alive.
    peer: Option<SocketAddr>,
    seq: Option<u32>,
    pressed: Buttons,
    released: Buttons,
    /// Receiver counters since start, wrapping — a sender reads their change,
    /// never their value (`pad-map/src/wire.rs:204-205`).
    accepted: u32,
    stale: u32,
    /// Datagrams that were not pad state at all, from anyone.
    malformed: u32,
    /// A second sender is told so at most once a [`BUSY_EVERY`]
    /// (`netpadd/src/main.rs:444-489`).
    busy_answered: Option<Instant>,
    /// The last answer, for the caller that sends it.
    answer: Option<(SocketAddr, Status)>,
    /// The caller's say in the DKST verdict: whether the duck is currently being
    /// driven by the pad (`netpadd/src/main.rs:459-477` maps accept to Driving,
    /// but the driving/belief decision here belongs to the shell — the pad can
    /// hold the link while the shell is not taking twist from it).
    driving: bool,
    /// Button bits this build does not know, OR-ed over everything accepted.
    reserved_seen: u16,
    drops: u64,
}

impl Shared {
    /// One datagram in, everything the wire asks of the receiver out
    /// (`netpadd/src/receiver.rs:114-171`, the subset without the clock
    /// judgement). `now` stamps freshness; the verdict stays `driving`'s.
    fn on_datagram(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> Accept {
        let frame = match decode_packet(bytes) {
            Ok(f) => f,
            Err(_) => {
                self.malformed = self.malformed.wrapping_add(1);
                self.drops += 1;
                // A malformed datagram is not answered at all
                // (`docs/robot/udp-pad.md:268`) — and does not re-arm the
                // previous datagram's answer either.
                self.answer = None;
                return Accept::Malformed;
            }
        };
        self.reserved_seen |= frame.reserved();
        // Newer, in wrapping order, and not implausibly so, from whoever sent
        // the last one (`receiver.rs:121-128`).
        let ahead = frame
            .seq
            .wrapping_sub(self.seq.unwrap_or(frame.seq.wrapping_sub(1)));
        let continues = self.peer == Some(from) && (1..=MAX_SEQ_AHEAD).contains(&ahead);
        match self.peer {
            Some(peer) if peer != from => {
                // A second sender hears why it is ignored — at most once a
                // [`BUSY_EVERY`], so the robot never answers every datagram of
                // a client it is not listening to
                // (`netpadd/src/main.rs:468-477`).
                self.drops += 1;
                if self
                    .busy_answered
                    .is_some_and(|at| now.duration_since(at) < BUSY_EVERY)
                {
                    return Accept::OtherPeer;
                }
                self.busy_answered = Some(now);
                self.answer = Some((from, self.status(Verdict::Busy, frame.seq)));
                return Accept::OtherPeer;
            }
            Some(_) if !continues => {
                // Stale from the driving client still counts as stale
                // (`netpadd/src/main.rs:464-467`) and IS answered (the doc:
                // "stale ones included").
                self.stale = self.stale.wrapping_add(1);
                self.drops += 1;
                self.answer = Some((from, self.status(Verdict::Driving, frame.seq)));
                return Accept::Stale;
            }
            _ => {}
        }
        let connected = if self.peer.is_some() {
            let (now_held, before) = (
                frame.held(),
                self.frame.map(|f| f.held()).unwrap_or(Buttons::NONE),
            );
            self.pressed |= now_held.newly_set(before);
            self.released |= before.newly_set(now_held);
            false
        } else {
            // A new client, or the old one back: what it holds was pressed
            // before we saw it, and firing it now would run a skill nobody just
            // asked for (`receiver.rs:147-160`).
            self.peer = Some(from);
            self.pressed = Buttons::NONE;
            self.released = Buttons::NONE;
            true
        };
        self.seq = Some(frame.seq);
        self.frame = Some(frame);
        self.last = Some(now);
        self.accepted = self.accepted.wrapping_add(1);
        self.answer = Some((
            from,
            self.status(
                if self.driving {
                    Verdict::Driving
                } else {
                    Verdict::Busy
                },
                frame.seq,
            ),
        ));
        if connected {
            Accept::Connected
        } else {
            Accept::Taken
        }
    }

    /// The answer for one datagram (`netpadd/src/main.rs:450-489`): accepted and
    /// stale both answer Driving; the counters are the receiver's own.
    fn status(&self, verdict: Verdict, seq: u32) -> Status {
        Status {
            verdict,
            seq,
            accepted: self.accepted,
            stale: self.stale,
            malformed: self.malformed,
        }
    }
}

/// The receiver half. One socket, one listener thread, one shared snapshot.
/// Dropping the handle does not kill the OS thread until the process ends,
/// so `stop()` explicitly flips the gate — no frames are consumed then.
/// The same socket answers DKPD with DKST and carries DKRB to the driving
/// client: one port, one thread, exactly the official shape.
#[derive(Clone)]
pub struct Pad {
    shared: Arc<Mutex<Shared>>,
    enabled: Arc<std::sync::atomic::AtomicBool>,
    socket: Arc<UdpSocket>,
}

impl Pad {
    /// Bind 4210 and start listening. Off by default: `enable()` is a
    /// deliberate second step the UI takes with the pairing code.
    pub fn start() -> Option<Pad> {
        let socket = UdpSocket::bind(("0.0.0.0", PORT)).ok()?;
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok()?;
        let socket = Arc::new(socket);
        let shared = Arc::new(Mutex::new(Shared::default()));
        let enabled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let listener_shared = shared.clone();
        let listener_enabled = enabled.clone();
        let listener_socket = socket.clone();
        std::thread::Builder::new()
            .name("duck-netpad".to_owned())
            .spawn(move || {
                let mut buf = [0u8; 2048];
                loop {
                    match listener_socket.recv_from(&mut buf) {
                        Ok((count, from)) => {
                            if !listener_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                                continue;
                            }
                            if let Ok(mut state) = listener_shared.lock() {
                                state.on_datagram(from, &buf[..count], Instant::now());
                            }
                        }
                        Err(_) => continue,
                    }
                }
            })
            .ok()?;
        Some(Pad {
            shared,
            enabled,
            socket,
        })
    }

    pub fn set_enabled(&self, value: bool) {
        self.enabled
            .store(value, std::sync::atomic::Ordering::Relaxed);
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
        let fresh = state.last.is_some_and(|at| at.elapsed() <= STALE);
        if fresh {
            state.frame
        } else {
            state.frame = None;
            None
        }
    }

    /// The button edges accumulated since the last [`Self::take_edges`], scaled
    /// the official way into a [`PadFrame`]. Empty (`NONE`, `NONE`) when the pad
    /// is not driving or nothing changed; a press and a release that both land
    /// between two reads are both seen, because edges accumulate per datagram
    /// (`netpadd/src/receiver.rs:147-160`, `docs/robot/udp-pad.md:318-320`).
    pub fn take_edges(&self) -> (Buttons, Buttons) {
        if !self.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            return (Buttons::NONE, Buttons::NONE);
        }
        let mut state = self.shared.lock().ok();
        let Some(state) = state.as_mut() else {
            return (Buttons::NONE, Buttons::NONE);
        };
        let (pressed, released) = (state.pressed, state.released);
        state.pressed = Buttons::NONE;
        state.released = Buttons::NONE;
        (pressed, released)
    }

    /// Tell the receiver whether the duck is currently being driven by this pad.
    /// The verdict a DKST carries: `Driving` while the shell takes the twist and
    /// the buttons, `Busy` when it does not (`pad-map/src/wire.rs:176-184`).
    pub fn set_driving(&self, driving: bool) {
        if let Ok(mut state) = self.shared.lock() {
            state.driving = driving;
        }
    }

    /// The DKST answer the last datagram produced, with the address to send it
    /// to. `None` when there is nothing to answer (malformed datagrams are not
    /// answered at all — `docs/robot/udp-pad.md:268`, "a malformed datagram not
    /// at all"). The caller sends it; `answer_last` does it here.
    pub fn take_answer(&self) -> Option<(SocketAddr, Status)> {
        let mut state = self.shared.lock().ok()?;
        state.answer.take()
    }

    /// Send the answer from the last datagram, best effort: a full buffer or an
    /// unreachable client drops it, never delays the listener
    /// (`netpadd/src/main.rs:224-228`).
    pub fn answer_last(&self) {
        if let Some((to, status)) = self.take_answer() {
            let _ = self.socket.send_to(&status.encode(), to);
        }
    }

    /// Build and send the DKRB report for the driving client, if one holds the
    /// duck. Once a second (`REPORT_EVERY`) and at once when a client takes the
    /// robot (`docs/robot/udp-pad.md:291-294`); the caller calls this on its own
    /// cadence and passes `connected` to get the at-once behaviour. Facts come
    /// from the caller ([`PadReportInput`]); the refusal from the shared
    /// [`Refused`] memory, filtered on [`REFUSAL_KEPT`].
    pub fn send_report(&self, input: &PadReportInput, refused: &Refused, now: Instant) {
        let Some(peer) = self.shared.lock().ok().and_then(|state| state.peer) else {
            return;
        };
        let report = RobotReport {
            health: input.health.clone(),
            health_error: input.health_error.clone(),
            posture: input.posture.clone(),
            posture_error: input.posture_error.clone(),
            refused: refused.refusal(now),
        };
        // As the answers are: never waited on, dropped when it cannot go
        // (`netpadd/src/main.rs:316-323`).
        let _ = self.socket.send_to(&report.encode(), peer);
    }

    /// Whether the driving client is owed a report now: the 1 s period is due, or
    /// a client just connected (the "at once on connecting" half of
    /// `docs/robot/udp-pad.md:291-294`). Returns `connected` so the caller can
    /// react to a new client in the same poll.
    pub fn report_due(&self, connected: Accept, last: Option<Instant>, now: Instant) -> bool {
        if matches!(connected, Accept::Connected) {
            return true;
        }
        last.is_none_or(|at| now.duration_since(at) >= REPORT_EVERY)
    }

    /// The address of the client that holds the duck, while there is one.
    pub fn peer(&self) -> Option<SocketAddr> {
        self.shared.lock().ok().and_then(|state| state.peer)
    }

    /// Button bits this build does not know, OR-ed over everything accepted —
    /// for one log line (`netpadd/src/receiver.rs:237-240`).
    pub fn reserved_bits_seen(&self) -> u16 {
        self.shared.lock().map_or(0, |state| state.reserved_seen)
    }

    /// The client's whole state, ready for the shell: the fresh frame (None when
    /// the pad is off or stale), plus the edges as a [`PadFrame`] scaled the
    /// official way. One call instead of two for the tick loop.
    pub fn tick_frame(&self) -> Option<PadFrame> {
        let frame = self.latest()?;
        let (pressed, released) = self.take_edges();
        Some(frame.pad_frame(pressed, released))
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Datagrams dropped for any reason — malformed, stale, other peer — since
    /// start. The shell's old counter, kept.
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

    /// The official DKPD encoder, transcribed from
    /// `pad-map/src/wire.rs:65-77` (`Packet::encode`) — the spec this module's
    /// parser must read. Written independently of `decode_packet` so the two
    /// cannot agree by sharing a bug.
    fn official_encode(seq: u32, t_ms: u32, axes: [i16; 6], buttons: u16, flags: u8) -> [u8; 28] {
        let mut b = [0u8; 28];
        b[..4].copy_from_slice(b"DKPD");
        b[4] = 1;
        b[5] = flags;
        b[6..10].copy_from_slice(&seq.to_le_bytes());
        b[10..14].copy_from_slice(&t_ms.to_le_bytes());
        for (i, a) in axes.iter().enumerate() {
            b[14 + 2 * i..16 + 2 * i].copy_from_slice(&a.to_le_bytes());
        }
        b[26..28].copy_from_slice(&buttons.to_le_bytes());
        b
    }

    /// `pad-map/src/wire.rs`'s sample packet (`wire.rs:431-439`), encoded
    /// officially.
    fn sample() -> [u8; 28] {
        official_encode(
            0xDEAD_BEEF,
            42,
            [32767, -32767, 0, 100, 32767, 0],
            0b1_0100_0001,
            SENDER_SOUNDS,
        )
    }

    #[test]
    fn the_official_twenty_eight_bytes_parse() {
        let frame = parse(&packet(7, 16384, 1)).expect("a DKPD frame");
        assert_eq!(frame.seq, 7);
        assert!((frame.axes.ly - 16384.0).abs() < 0.001);
        assert_eq!(frame.buttons, 1);
        // Extra tail is allowed for version 1.
        let mut longer = packet(8, 0, 0);
        longer.push(0xff);
        assert!(parse(&longer).is_some());
        // Wrong magic, wrong version, short packet: noise.
        assert!(parse(&[0u8; 28]).is_none());
        assert!(parse(&packet(9, 0, 0)[..27]).is_none());
    }

    /// Every field of the official sample decodes at its documented offset —
    /// `docs/robot/udp-pad.md:202-210` and `wire.rs:431-439` agree, and so must
    /// this reader.
    #[test]
    fn the_layout_is_little_endian_at_the_documented_offsets() {
        let b = sample();
        let f = parse(&b).expect("the official sample");
        assert_eq!(f.seq, 0xDEAD_BEEF);
        assert_eq!(f.t_ms, 42);
        assert_eq!(f.axes.lx, 32767.0);
        assert_eq!(f.axes.ly, -32767.0);
        assert_eq!(f.axes.rx, 0.0);
        assert_eq!(f.axes.ry, 100.0);
        assert_eq!(f.axes.lt, 32767.0);
        assert_eq!(f.axes.rt, 0.0);
        assert_eq!(u16::from_le_bytes([b[26], b[27]]), 0b1_0100_0001);
        assert!(f.sender_sounds, "flags bit 0");
        // Bits 0, 6 and 8: A, Start, Up (the upstream sample, `wire.rs:431-439`).
        assert!(f.held().contains(Buttons::A | Buttons::START | Buttons::UP));
        assert_eq!(f.reserved(), 0, "every bit of the sample is known");
    }

    /// A sender that knows nothing of the flags sends 0, and the duck sounds as
    /// it always has (`wire.rs:463-468`).
    #[test]
    fn flags_zero_leaves_the_sound_to_the_robot() {
        let f = parse(&official_encode(1, 0, [0; 6], 0, 0)).unwrap();
        assert!(!f.sender_sounds);
    }

    /// Sticks ±32767 to ±1 clamped, triggers never negative (`wire.rs:117-134`,
    /// pinned by upstream's `i16_min_clamps_to_minus_one`, `wire.rs:490-499`).
    #[test]
    fn i16_min_clamps_to_minus_one() {
        let f = parse(&official_encode(
            1,
            0,
            [i16::MIN, 0, 0, 0, i16::MIN, 0],
            0,
            0,
        ))
        .unwrap();
        let p = f.pad_frame(Buttons::NONE, Buttons::NONE);
        assert_eq!(p.left_x, -1.0);
        assert_eq!(p.lt, 0.0, "a trigger is never negative");
    }

    /// The DKST byte layout, asserted field by field against
    /// `wire.rs:220-230` (`Status::encode`) and the doc table at
    /// `udp-pad.md:272-282`.
    #[test]
    fn a_status_encodes_at_the_documented_offsets() {
        let s = Status {
            verdict: Verdict::Busy,
            seq: 0xDEAD_BEEF,
            accepted: 7,
            stale: 2,
            malformed: 1,
        };
        let b = s.encode();
        assert_eq!(b.len(), 24);
        assert_eq!(&b[..4], b"DKST");
        assert_eq!(b[4], 1);
        assert_eq!(b[5], 2, "Busy is verdict code 2 (`wire.rs:189-193`)");
        assert_eq!(
            u32::from_le_bytes(b[6..10].try_into().unwrap()),
            0xDEAD_BEEF
        );
        assert_eq!(u32::from_le_bytes(b[10..14].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(b[14..18].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(b[18..22].try_into().unwrap()), 1);
        assert_eq!(&b[22..], &[0, 0], "reserved");
        // Round trip, and the unknown-verdict rule (`wire.rs:363-370`).
        assert_eq!(Status::decode(&b), Ok(s));
        let mut odd = b;
        odd[5] = 9;
        assert_eq!(Status::decode(&odd).unwrap().verdict, Verdict::Other(9));
        assert_eq!(
            Status::decode(&sample()),
            Err(WireError::Magic),
            "a pad datagram is not a status"
        );
    }

    /// The driving client's answer: verdict Driving, the seq it answers, the
    /// receiver's counters. An expired (stale) datagram is answered too, with
    /// the same verdict and a bumped stale count — `netpadd/src/main.rs:464-467`
    /// and the doc's "stale ones included".
    #[test]
    fn the_driving_client_hears_each_seq_and_its_stale_ones() {
        let mut state = Shared::default();
        state.driving = true;
        let from = "10.0.0.2:5000".parse().unwrap();
        let now = Instant::now();

        let accept = state.on_datagram(from, &packet(10, 0, 0), now);
        assert_eq!(accept, Accept::Connected);
        let (to, s) = state.answer.take().expect("answered");
        assert_eq!(to, from);
        assert_eq!((s.verdict, s.seq), (Verdict::Driving, 10));
        assert_eq!((s.accepted, s.stale, s.malformed), (1, 0, 0));

        state.on_datagram(from, &packet(11, 0, 0), now);
        state.on_datagram(from, &packet(12, 0, 0), now);
        // A stale one: still Driving, still answered.
        let accept = state.on_datagram(from, &packet(11, 0, 0), now);
        assert_eq!(accept, Accept::Stale);
        let (_, s) = state.answer.take().expect("a stale one is answered too");
        assert_eq!((s.verdict, s.seq), (Verdict::Driving, 11));
        assert_eq!((s.accepted, s.stale), (3, 1));
    }

    /// A second sender hears Busy — at most once a second
    /// (`netpadd/src/main.rs:468-477`) — and a malformed datagram is counted but
    /// never answered (`docs/robot/udp-pad.md:268`).
    #[test]
    fn another_sender_hears_busy_and_malformed_is_counted_not_answered() {
        let mut state = Shared::default();
        state.driving = true;
        let (a, b) = (
            "10.0.0.2:5000".parse().unwrap(),
            "10.0.0.3:5000".parse().unwrap(),
        );
        let now = Instant::now();
        state.on_datagram(a, &packet(1, 0, 0), now);

        state.on_datagram(b, &packet(99, 0, 0), now);
        let (to, s) = state.answer.take().expect("busy");
        assert_eq!(to, b);
        assert_eq!(s.verdict, Verdict::Busy);
        assert_eq!(s.seq, 99);

        // Inside the busy window: still counted, but no answer built.
        state.on_datagram(b, &packet(100, 0, 0), now + Duration::from_millis(500));
        assert!(state.answer.take().is_none(), "once a second");

        state.on_datagram(b, &packet(101, 0, 0), now + Duration::from_millis(1001));
        assert!(state.answer.take().is_some(), "a second has passed");

        state.on_datagram(a, b"hello", now);
        assert!(state.answer.take().is_none(), "malformed is not answered");
        assert_eq!(state.malformed, 1);
    }

    /// The verdict follows the shell's say, not the socket's: a pad holding the
    /// link while the shell is not driving hears Busy (`wire.rs:176-184`).
    #[test]
    fn the_verdict_is_the_callers_driving_say() {
        let mut state = Shared::default();
        let from = "10.0.0.2:5000".parse().unwrap();
        state.on_datagram(from, &packet(1, 0, 0), Instant::now());
        let (_, s) = state.answer.take().unwrap();
        assert_eq!(s.verdict, Verdict::Busy, "not driving by default (Other)");

        state.driving = true;
        state.on_datagram(from, &packet(2, 0, 0), Instant::now());
        let (_, s) = state.answer.take().unwrap();
        assert_eq!(s.verdict, Verdict::Driving);
    }

    /// Edges accumulate across datagrams and are consumed once: a press and a
    /// release that both land between two reads are both seen
    /// (`receiver.rs:147-160`, doc lines 318-320).
    #[test]
    fn a_press_and_release_inside_one_read_are_both_seen() {
        let mut state = Shared::default();
        let from = "10.0.0.2:5000".parse().unwrap();
        let now = Instant::now();
        state.on_datagram(from, &packet(1, 0, 0), now);
        assert_eq!(state.answer.take().unwrap().1.verdict, Verdict::Busy);
        // (The first client takes the lock even when the shell is not driving —
        // `receiver.rs` locks on first accept; Busy is the verdict only.)

        state.on_datagram(from, &packet(2, 0, Buttons::START.0), now);
        state.on_datagram(from, &packet(3, 0, 0), now);
        let (pressed, released) = (state.pressed, state.released);
        assert!(pressed.contains(Buttons::START) && released.contains(Buttons::START));
    }

    /// The first datagram makes no edges (`receiver.rs:445-451`): what the
    /// client holds was pressed before we saw it.
    #[test]
    fn the_first_datagram_makes_no_edges() {
        let mut state = Shared::default();
        state.on_datagram(
            "10.0.0.2:5000".parse().unwrap(),
            &packet(1, 0, Buttons::A.0 | Buttons::UP.0),
            Instant::now(),
        );
        assert_eq!(
            (state.pressed, state.released),
            (Buttons::NONE, Buttons::NONE)
        );
    }

    /// The DKRB layout: magic, version, then JSON — asserted against the
    /// official struct's shape (`wire.rs:324-330`, doc lines 291-308). Every
    /// field optional; absent is *not known*, never zero.
    #[test]
    fn a_robot_report_is_magic_version_then_json() {
        let r = RobotReport {
            health: Some(duck_ipc_proto::HealthResult {
                healthy: true,
                battery: Some(duck_ipc_proto::Battery {
                    volts: 7.62,
                    percent: 61.0,
                }),
                ..Default::default()
            }),
            posture: Some(Posture {
                policy: "walk".to_owned(),
                fallen: false,
                limp: false,
                picked_up: false,
            }),
            ..Default::default()
        };
        let b = r.encode();
        assert_eq!(&b[..5], b"DKRB\x01");
        let text = std::str::from_utf8(&b[5..]).unwrap();
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["health"]["healthy"], serde_json::json!(true));
        assert_eq!(v["health"]["battery"]["volts"], serde_json::json!(7.62));
        assert_eq!(v["posture"]["policy"], serde_json::json!("walk"));
        assert!(v.get("refused").is_none(), "absent is not zero");
        assert_eq!(RobotReport::decode(&b).unwrap(), r);
        // An empty report is every field unknown, not zero (`wire.rs:427-428`).
        assert_eq!(
            RobotReport::decode(b"DKRB\x01{}").unwrap(),
            RobotReport::default()
        );
        assert!(RobotReport::decode(&sample()).is_err());
    }

    /// The refusal memory: reported with `ago_ms` counting from when it
    /// happened, kept exactly `REFUSAL_KEPT`, dropped after
    /// (`netpadd/src/main.rs:503-509`).
    #[test]
    fn a_refusal_is_kept_thirty_seconds_and_reports_its_age() {
        let t0 = Instant::now();
        let mut refused = Refused::default();
        assert!(refused.refusal(t0).is_none(), "nothing refused yet");
        refused.remember_refused("robot.do", "not home yet", t0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let r = refused.refusal(at(1_234)).expect("kept");
        assert_eq!(
            (r.method.as_str(), r.reason.as_str()),
            ("robot.do", "not home yet")
        );
        assert_eq!(r.ago_ms, 1_234);
        assert!(refused.refusal(at(29_999)).is_some(), "still inside 30 s");
        assert!(refused.refusal(at(30_000)).is_none(), "dropped at 30 s");
        // The wire carries only the LAST refusal.
        refused.remember_refused("robot.enable", "shutting down", at(1_000));
        let r = refused.refusal(at(2_000)).unwrap();
        assert_eq!(r.method, "robot.enable");
    }

    /// The official default bindings, read from the official type — nothing
    /// re-typed here (`robotd-params/src/lib.rs:416-427`).
    #[test]
    fn the_default_bindings_are_the_official_ones() {
        let m = PadMapper::new();
        assert_eq!(m.skill("a"), Some("sit_toggle"));
        assert_eq!(m.skill("b"), Some("ground_pick"));
        assert_eq!(m.skill("x"), Some(""));
        assert_eq!(m.skill("y"), Some("excited"));
        assert_eq!(m.skill("lb"), Some("yes"));
        assert_eq!(m.skill("rb"), Some("no"));
        assert_eq!(m.skill("start"), None);
        // A rebind reaches the table and only it.
        let mut m = PadMapper::new();
        assert!(m.bind("x", "roulade"));
        assert_eq!(m.skill("x"), Some("roulade"));
        assert!(
            !m.bind("home", "x"),
            "Home is not bindable (`mapper.rs:330`)"
        );
    }

    /// A frame becomes the events the official mapping asks for: A runs its
    /// binding on the press edge only, Home toggles the flashlight once per
    /// press, the D-pad changes the mode once (`mapper.rs:575-619` tests, and
    /// `mapper.rs:206-219`).
    #[test]
    fn a_frame_becomes_the_official_events() {
        let (mut m, t) = (PadMapper::new(), Instant::now());
        let mut events = Vec::new();
        let base = PadFrame {
            left_y: 1.0,
            ..Default::default()
        };
        m.tick(&base, t, &mut events);
        assert!(events.is_empty(), "sticks are continuous, not events");

        // A on its edge.
        let press = PadFrame {
            held: Buttons::A,
            pressed: Buttons::A,
            ..Default::default()
        };
        m.tick(&press, t, &mut events);
        assert_eq!(events, vec![PadButtonEvent::Skill("a")]);
        // Held is not a second press (`mapper.rs:605-618`).
        m.tick(&base, t, &mut events);
        assert!(events.is_empty());

        // Home once per press.
        let home = PadFrame {
            held: Buttons::HOME,
            pressed: Buttons::HOME,
            ..Default::default()
        };
        m.tick(&home, t, &mut events);
        assert_eq!(events, vec![PadButtonEvent::Flashlight]);
        m.tick(
            &PadFrame {
                held: Buttons::HOME,
                ..Default::default()
            },
            t,
            &mut events,
        );
        assert!(events.is_empty(), "held is not a second press");

        // D-pad up: head mode, once, then quiet while held.
        let up = PadFrame {
            held: Buttons::UP,
            pressed: Buttons::UP,
            ..Default::default()
        };
        m.tick(&up, t, &mut events);
        assert_eq!(events, vec![PadButtonEvent::ModeChange(PadMode::Head)]);
        assert_eq!(m.mode(), PadMode::Head);
        m.tick(
            &PadFrame {
                held: Buttons::UP,
                ..Default::default()
            },
            t,
            &mut events,
        );
        assert!(events.is_empty(), "still head mode, no new event");
    }

    /// Start's tap is Init while the pad believes the robot not up, the toggle
    /// once it is, and a 1.5 s hold is the way home — never also a tap
    /// (`mapper.rs:227-281`, `hold.rs` tests).
    #[test]
    fn start_taps_init_then_toggles_and_a_long_hold_goes_home_only() {
        let (mut m, t) = (PadMapper::new(), Instant::now());
        let at = |ms: u64| t + Duration::from_millis(ms);
        let mut events = Vec::new();
        let start = PadFrame {
            held: Buttons::START,
            pressed: Buttons::START,
            ..Default::default()
        };
        let held = PadFrame {
            held: Buttons::START,
            ..Default::default()
        };
        let release = PadFrame {
            released: Buttons::START,
            ..Default::default()
        };

        m.tick(&start, at(0), &mut events);
        // A tap is the RELEASE edge (`hold.rs:80`): nothing on the press.
        assert!(events.is_empty());
        m.tick(&release, at(200), &mut events);
        assert_eq!(
            events,
            vec![PadButtonEvent::Init],
            "the tap stands the robot up"
        );
        assert!(events.is_empty() || events.len() == 1);
        events.clear();
        assert!(
            events.is_empty(),
            "the release after a tap says nothing more"
        );

        // The caller reports the robot stood up: the next tap is the toggle.
        m.set_up(true);
        m.tick(&start, at(1_000), &mut events);
        assert!(events.is_empty());
        m.tick(&release, at(1_200), &mut events);
        assert_eq!(events, vec![PadButtonEvent::Toggle]);

        // Held past 1.5 s: the way home, once, and the release is not a tap.
        m.tick(&held, at(2_000), &mut events);
        m.tick(&held, at(3_490), &mut events);
        assert!(events.is_empty());
        m.tick(&held, at(3_500), &mut events);
        assert_eq!(events, vec![PadButtonEvent::Home]);
        assert_eq!(
            m.mode(),
            PadMode::Drive,
            "home resets the mode (`mapper.rs:271`)"
        );
        m.tick(&release, at(9_020), &mut events);
        assert!(events.is_empty());
    }

    /// Select: the rest at 2 s without waiting for the release, the power-off at
    /// 4 s, the release asking nothing (`mapper.rs:357-388`, pinned by
    /// `hold.rs`'s own tests).
    #[test]
    fn select_rests_at_two_seconds_and_powers_off_at_four() {
        let (mut m, t) = (PadMapper::new(), Instant::now());
        let at = |ms: u64| t + Duration::from_millis(ms);
        let mut events = Vec::new();
        let held = PadFrame {
            held: Buttons::SELECT,
            ..Default::default()
        };
        let up = PadFrame {
            released: Buttons::SELECT,
            ..Default::default()
        };

        let down = PadFrame {
            held: Buttons::SELECT,
            pressed: Buttons::SELECT,
            ..Default::default()
        };
        m.tick(&down, at(0), &mut events);
        assert!(events.is_empty());
        m.tick(&held, at(1_900), &mut events);
        assert!(events.is_empty(), "not yet two seconds");
        m.tick(&held, at(2_000), &mut events);
        assert_eq!(events, vec![PadButtonEvent::Rest]);
        m.tick(&held, at(3_000), &mut events);
        assert!(events.is_empty(), "the rest is sent once");
        m.tick(&held, at(4_000), &mut events);
        assert_eq!(events, vec![PadButtonEvent::Shutdown]);
        m.tick(&up, at(4_100), &mut events);
        assert!(events.is_empty(), "the release asks nothing");
    }

    /// RT chirps on its rising edge only; the wheee rides LT and is cut on
    /// release (`mapper.rs:399-422`).
    #[test]
    fn the_triggers_chirp_and_ride() {
        let (mut m, t) = (PadMapper::new(), Instant::now());
        let mut events = Vec::new();
        let triggers = |lt: f64, rt: f64| PadFrame {
            lt,
            rt,
            ..Default::default()
        };
        m.tick(&triggers(0.0, 0.2), t, &mut events);
        assert!(events.is_empty(), "below the threshold");
        m.tick(&triggers(0.0, 0.3), t, &mut events);
        assert_eq!(events, vec![PadButtonEvent::Chirp], "the crossing");
        m.tick(&triggers(0.0, 1.0), t, &mut events);
        assert!(events.is_empty(), "no second chirp while held");
        m.tick(&triggers(0.5, 0.0), t, &mut events);
        // The wheee is a HOLD: re-sent per tick while LT stays down, the way
        // `mapper.rs:410-414` sends `hold: Some(true)` every tick — the robot
        // treats the hold as a level that decays.
        assert_eq!(events, vec![PadButtonEvent::Wheee]);
        m.tick(&triggers(0.5, 0.0), t, &mut events);
        assert_eq!(
            events,
            vec![PadButtonEvent::Wheee],
            "a hold notification per tick"
        );
        m.tick(&triggers(0.0, 0.0), t, &mut events);
        assert_eq!(events, vec![PadButtonEvent::WheeeOff]);
    }

    /// A hold does not survive the pad going away (`hold.rs:184-286`): reset on
    /// dropout, the tail does nothing, and the release is not a tap.
    #[test]
    fn a_hold_does_not_survive_the_pad_going_away() {
        let (mut m, t) = (PadMapper::new(), Instant::now());
        let at = |ms: u64| t + Duration::from_millis(ms);
        let mut events = Vec::new();
        let select = PadFrame {
            held: Buttons::SELECT,
            pressed: Buttons::SELECT,
            ..Default::default()
        };
        m.tick(&select, at(0), &mut events);
        m.pad_gone();
        // Back three seconds later with Select still down.
        let still_down = PadFrame {
            held: Buttons::SELECT,
            ..Default::default()
        };
        m.tick(&still_down, at(3_000), &mut events);
        m.tick(
            &still_down,
            at(3_000).checked_add(REST_HOLD).unwrap(),
            &mut events,
        );
        assert!(
            events.is_empty(),
            "a hold begun against the pad that left does not rest: {events:?}"
        );
    }

    /// `Frame::pad_frame` carries the edges in and the held set through, the
    /// way `wire.rs:117-134` builds `PadFrame`.
    #[test]
    fn pad_frame_carries_held_pressed_released() {
        let f = parse(&packet(1, 0, Buttons::A.0 | Buttons::START.0)).unwrap();
        let p = f.pad_frame(Buttons::B, Buttons::HOME);
        assert!(p.held.contains(Buttons::A | Buttons::START));
        assert!(p.pressed.contains(Buttons::B));
        assert!(p.released.contains(Buttons::HOME));
    }
}

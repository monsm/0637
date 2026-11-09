import io

p = 'core/duck-mobile-core/src/runtime.rs'
s = io.open(p, encoding='utf-8').read()

struct = '''/// Everything a rewind has to put back, as one value.
///
/// The body state is MuJoCo's own integration state plus the actuator
/// bookkeeping; the rest is the control loop's: the tick, the filters, the
/// skill in flight, the policy's recurrent memory, the safety debounces and
/// the limp-fall phase. A checkpoint that misses any of them looks right and
/// then diverges, which is why this is one struct and not a collection of
/// setters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    pub tick: u64,
    pub speed: f64,
    pub paused: bool,
    pub body: Vec<f64>,
    pub policy_state: Vec<u8>,
    pub active_net: Option<duck_control::policy::Net>,
    pub controller: crate::control::ControllerState,
    pub falling_ms: u64,
    pub fallen: bool,
    pub fall_falling_ms: u64,
    pub fall_upright_ms: u64,
    pub fall_armed: bool,
    pub bringup: String,
    pub homing_from: Option<Vec<f64>>,
    pub homing_since: u64,
    pub homing_to: Option<Vec<f64>>,
    pub limp_fall: String,
    pub limp_since: u64,
    pub limp_still_ms: u64,
    pub pose_from: Option<Vec<f64>>,
    pub enabled: bool,
    pub command_twist: [f64; 3],
    pub command_head: [f64; 4],
    pub command_body: [f64; 3],
    pub twist_ema: [f64; 3],
    pub head_ema: [f64; 4],
    pub body_ema: [f64; 3],
    pub body_active: bool,
    pub twist_fresh: bool,
    pub last_twist_tick: u64,
    pub last_head_tick: u64,
    pub lease_twist: [f64; 3],
    pub lease_until: Option<u64>,
}

/// The runtime. `T` is the body: MuJoCo in the app, `RemoteIo` on the desktop harness.'''

anchor = '/// The runtime. `T` is the body: MuJoCo in the app, `RemoteIo` on the desktop harness.'
assert anchor in s
s = s.replace(anchor, struct, 1)

methods = '''    /// Everything needed to resume from this tick, bit for bit.
    pub fn checkpoint(&self) -> Option<Checkpoint> {
        let body = self.safety.snapshot()?;
        let (falling_ms, fallen) = self.safety.fall_state();
        let (fall_falling_ms, fall_upright_ms, fall_armed) = self.fall.state();
        let (bringup, homing_from, homing_since, homing_to) = match self.bringup {
            Bringup::Limp => ("limp".to_owned(), None, 0, None),
            Bringup::Homing { from, since, to } => (
                "homing".to_owned(),
                Some(from.to_vec()),
                since.0,
                Some(to.to_vec()),
            ),
            Bringup::Ready => ("ready".to_owned(), None, 0, None),
        };
        let (limp_fall, limp_since, limp_still_ms, pose_from) = match self.limp_fall {
            LimpFall::Idle => ("idle".to_owned(), 0, 0, None),
            LimpFall::Limp { since, landing } => (
                "limp".to_owned(),
                since.0,
                landing.still_for.as_millis() as u64,
                None,
            ),
            LimpFall::Posing { from, since } => (
                "posing".to_owned(),
                since.0,
                0,
                Some(from.to_vec()),
            ),
        };
        Some(Checkpoint {
            tick: self.clock.tick,
            speed: self.clock.speed,
            paused: self.clock.paused,
            body,
            policy_state: self.controller.policy_state(),
            active_net: self.controller.active_net(),
            controller: self.controller.snapshot(),
            falling_ms,
            fallen,
            fall_falling_ms,
            fall_upright_ms,
            fall_armed,
            bringup,
            homing_from,
            homing_since,
            homing_to,
            limp_fall,
            limp_since,
            limp_still_ms,
            pose_from,
            enabled: self.enabled,
            command_twist: self.command.twist,
            command_head: self.command.head,
            command_body: [
                self.command.body.z,
                self.command.body.roll,
                self.command.body.pitch,
            ],
            twist_ema: self.twist_ema,
            head_ema: self.head_ema,
            body_ema: self.body_ema,
            body_active: self.body_active,
            twist_fresh: self.twist_fresh,
            last_twist_tick: self.last_twist.0,
            last_head_tick: self.last_head_command.0,
            lease_twist: self.api_lease.twist,
            lease_until: self.api_lease.until.map(|until| until.0),
        })
    }

    /// Resume from [`Self::checkpoint`]. False when the body cannot be rewound
    /// or the state does not belong to this build.
    pub fn restore(&mut self, checkpoint: &Checkpoint) -> bool {
        if !self.safety.restore(&checkpoint.body) {
            return false;
        }
        if !self
            .controller
            .restore_policy_state(checkpoint.active_net, &checkpoint.policy_state)
        {
            return false;
        }
        self.controller.restore(&checkpoint.controller);
        self.safety
            .set_fall_state(checkpoint.falling_ms, checkpoint.fallen);
        self.fall.set_state(
            checkpoint.fall_falling_ms,
            checkpoint.fall_upright_ms,
            checkpoint.fall_armed,
        );
        self.clock.tick = checkpoint.tick;
        self.clock.speed = checkpoint.speed;
        self.clock.paused = checkpoint.paused;
        self.bringup = match checkpoint.bringup.as_str() {
            "homing" => Bringup::Homing {
                from: array_of(checkpoint.homing_from.as_deref()),
                since: TickTime(checkpoint.homing_since),
                to: array_of(checkpoint.homing_to.as_deref()),
            },
            "ready" => Bringup::Ready,
            _ => Bringup::Limp,
        };
        self.limp_fall = match checkpoint.limp_fall.as_str() {
            "limp" => LimpFall::Limp {
                since: TickTime(checkpoint.limp_since),
                landing: Landing {
                    still_for: Duration::from_millis(checkpoint.limp_still_ms),
                },
            },
            "posing" => LimpFall::Posing {
                from: array_of(checkpoint.pose_from.as_deref()),
                since: TickTime(checkpoint.limp_since),
            },
            _ => LimpFall::Idle,
        };
        self.enabled = checkpoint.enabled;
        self.command.twist = checkpoint.command_twist;
        self.command.head = checkpoint.command_head;
        self.command.body.z = checkpoint.command_body[0];
        self.command.body.roll = checkpoint.command_body[1];
        self.command.body.pitch = checkpoint.command_body[2];
        self.twist_ema = checkpoint.twist_ema;
        self.head_ema = checkpoint.head_ema;
        self.body_ema = checkpoint.body_ema;
        self.body_active = checkpoint.body_active;
        self.twist_fresh = checkpoint.twist_fresh;
        self.last_twist = TickTime(checkpoint.last_twist_tick);
        self.last_head_command = TickTime(checkpoint.last_head_tick);
        self.api_lease.twist = checkpoint.lease_twist;
        self.api_lease.until = checkpoint.lease_until.map(TickTime);
        true
    }

    pub fn set_paused(&mut self, paused: bool) {'''

anchor2 = '    pub fn set_paused(&mut self, paused: bool) {'
assert anchor2 in s
s = s.replace(anchor2, methods, 1)

helper = '''fn array_of(values: Option<&[f64]>) -> [f64; NUM_JOINTS] {
    let mut out = [0.0; NUM_JOINTS];
    if let Some(values) = values {
        for (slot, value) in out.iter_mut().zip(values.iter()) {
            *slot = *value;
        }
    }
    out
}

/// A deadline on the simulation clock: "this tick, or later".'''
anchor3 = '/// A deadline on the simulation clock: "this tick, or later".'
assert anchor3 in s
s = s.replace(anchor3, helper, 1)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')

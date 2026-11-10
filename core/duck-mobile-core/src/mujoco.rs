//! In-process MuJoCo body behind the official `RobotIo` seam.
//!
//! The C shim (`native/duck_mj_shim.c`) hides MuJoCo's struct layout and version;
//! this module only maps wire joints to qpos/qvel/actuators and implements the
//! trait the control core expects. Physics is stepped inside `write`, four 5 ms
//! steps per 20 ms control tick — the same decimation as `infer_policy.py`.
//!
//! For the first on-device version the model uses the XML position actuators
//! (what the official `duck-body` twin uses). BAM is the next fidelity step.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};
use std::path::Path;
use std::slice;

use duck_control::imu::ImuData;
use duck_control::io::{JointTargets, Result, RobotIo, Sensors, SlowSensors};
use duck_control::model::{DEFAULT_POSITION, NUM_JOINTS};

use crate::bam::BamModel;

#[repr(C)]
struct DuckMj {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn duck_mj_load(path: *const c_char, error: *mut c_char, error_size: c_int) -> *mut DuckMj;
    fn duck_mj_free(handle: *mut DuckMj);

    fn duck_mj_nq(handle: *const DuckMj) -> c_int;
    fn duck_mj_nv(handle: *const DuckMj) -> c_int;
    fn duck_mj_nu(handle: *const DuckMj) -> c_int;
    fn duck_mj_nbody(handle: *const DuckMj) -> c_int;
    fn duck_mj_ngeom(handle: *const DuckMj) -> c_int;
    fn duck_mj_nmesh(handle: *const DuckMj) -> c_int;
    fn duck_mj_version() -> *const c_char;
    fn duck_mj_set_solver(handle: *mut DuckMj, iterations: c_int, ls_iterations: c_int);
    fn duck_mj_state_size(handle: *const DuckMj) -> c_int;
    fn duck_mj_state_get(handle: *const DuckMj, out: *mut c_double);
    fn duck_mj_state_set(handle: *mut DuckMj, input: *const c_double);
    fn duck_mj_clone(handle: *const DuckMj) -> *mut DuckMj;
    fn duck_mj_timestep(handle: *const DuckMj) -> c_double;
    fn duck_mj_set_timestep(handle: *mut DuckMj, dt: c_double);

    fn duck_mj_qpos(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_qvel(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_ctrl(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_actuator_force(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_xpos(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_xquat(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_geom_colour(handle: *const DuckMj, geom: c_int, out: *mut f32);
    fn duck_mj_geom_pos(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_geom_quat(handle: *mut DuckMj) -> *mut c_double;
    fn duck_mj_geom_type(handle: *const DuckMj, geom: c_int) -> c_int;
    fn duck_mj_geom_bodyid(handle: *const DuckMj, geom: c_int) -> c_int;
    fn duck_mj_geom_meshid(handle: *const DuckMj, geom: c_int) -> c_int;
    fn duck_mj_geom_group(handle: *const DuckMj, geom: c_int) -> c_int;
    fn duck_mj_mesh_name(handle: *const DuckMj, mesh: c_int) -> *const c_char;
    fn duck_mj_mesh_vert(handle: *const DuckMj) -> *const f32;
    fn duck_mj_mesh_vertadr(handle: *const DuckMj) -> *const c_int;
    fn duck_mj_mesh_vertnum(handle: *const DuckMj) -> *const c_int;
    fn duck_mj_mesh_face(handle: *const DuckMj) -> *const c_int;
    fn duck_mj_mesh_faceadr(handle: *const DuckMj) -> *const c_int;
    fn duck_mj_mesh_facenum(handle: *const DuckMj) -> *const c_int;

    fn duck_mj_joint_id(handle: *const DuckMj, name: *const c_char) -> c_int;
    fn duck_mj_actuator_id(handle: *const DuckMj, name: *const c_char) -> c_int;
    fn duck_mj_jnt_qposadr(handle: *const DuckMj, joint: c_int) -> c_int;
    fn duck_mj_jnt_dofadr(handle: *const DuckMj, joint: c_int) -> c_int;
    fn duck_mj_actuator_trnid(handle: *const DuckMj, actuator: c_int, index: c_int) -> c_int;

    fn duck_mj_set_position_gain(handle: *mut DuckMj, actuator: c_int, kp: c_double);
    fn duck_mj_position_gain(handle: *const DuckMj, actuator: c_int) -> c_double;
    fn duck_mj_make_motor(handle: *mut DuckMj, actuator: c_int);
    fn duck_mj_set_force_limit(handle: *mut DuckMj, actuator: c_int, limit: c_double);
    fn duck_mj_zero_joint_friction(
        handle: *mut DuckMj,
        joint: c_int,
        armature: c_double,
        solref0: c_double,
        solref1: c_double,
        solimp0: c_double,
        solimp1: c_double,
        solimp2: c_double,
        solimp3: c_double,
        solimp4: c_double,
    );
    fn duck_mj_set_dof_friction(handle: *mut DuckMj, dof: c_int, frictionloss: c_double, damping: c_double);
    fn duck_mj_external_torque(handle: *mut DuckMj, dof: c_int) -> c_double;
    fn duck_mj_actuator_torque(handle: *mut DuckMj, dof: c_int) -> c_double;
    fn duck_mj_set_ctrl(handle: *mut DuckMj, actuator: c_int, value: c_double);
    fn duck_mj_set_trunk_velocity(handle: *mut DuckMj, vx: c_double, vy: c_double, vz: c_double);
    fn duck_mj_set_trunk_force(handle: *mut DuckMj, fx: c_double, fy: c_double, fz: c_double);
    fn duck_mj_set_const(handle: *mut DuckMj);
    fn duck_mj_reset(handle: *mut DuckMj);
    fn duck_mj_forward(handle: *mut DuckMj);
    fn duck_mj_step(handle: *mut DuckMj);
    fn duck_mj_quat2mat(mat: *mut c_double, quat: *const c_double);
}

/// MuJoCo's version, for the provenance fingerprint.
pub fn version() -> String {
    unsafe {
        let pointer = duck_mj_version();
        if pointer.is_null() {
            return "unknown".to_owned();
        }
        std::ffi::CStr::from_ptr(pointer)
            .to_string_lossy()
            .into_owned()
    }
}

const WIRE_NAMES: [&str; NUM_JOINTS] = [
    "left_hip_yaw",
    "left_hip_roll",
    "left_hip_pitch",
    "left_knee",
    "left_ankle",
    "neck_pitch",
    "head_pitch",
    "head_yaw",
    "head_roll",
    "mouth",
    "right_hip_yaw",
    "right_hip_roll",
    "right_hip_pitch",
    "right_knee",
    "right_ankle",
];

const TRUNK_JOINT: &str = "trunk_base_freejoint";
const HOME_TRUNK_Z: f64 = 0.125;
const STEPS_PER_TICK: usize = 4;

/// The hand that carries the duck: a spring-damper on the trunk.
///
/// Stiff enough to hold 0.77 kg against gravity with a few millimetres of sag
/// (mg/k ≈ 2 cm at 350 N/m is too soft; 800 N/m sags 9 mm), damped at critical
/// for that mass, and capped well above the weight so a drag still feels direct.
const CARRY_STIFFNESS: f64 = 800.0;
const CARRY_DAMPING: f64 = 50.0;
const CARRY_MAX_FORCE: f64 = 60.0;

/// One renderable geom, as the app's Metal renderer needs it.
#[derive(Debug, Clone, Copy)]
pub struct GeomInfo {
    pub body: usize,
    pub geom_type: i32,
    pub mesh: i32,
    pub group: i32,
    pub pos: [f64; 3],
    pub quat: [f64; 4],
    pub rgba: [f32; 4],
}

/// BAM state for the torque-controlled path.
struct BamState {
    model: BamModel,
    q_target: Vec<f64>,
    previous_torque: Vec<f64>,
}

/// A MuJoCo model + data behind the official `RobotIo` trait.
pub struct MujocoIo {
    handle: *mut DuckMj,
    nq: usize,
    nv: usize,
    nu: usize,
    trunk_qpos: usize,
    trunk_dof: usize,
    /// Per wire joint: actuator index, qpos address, dof address.
    wire_actuator: [Option<usize>; NUM_JOINTS],
    wire_qpos: [Option<usize>; NUM_JOINTS],
    wire_dof: [Option<usize>; NUM_JOINTS],
    /// The passive backlash hinge in series with each servo. The encoder
    /// reads the OUTPUT side of the gear play, i.e. servo + backlash; the
    /// training env's BacklashEncoderBamActuator reads exactly that sum.
    wire_backlash_qpos: [Option<usize>; NUM_JOINTS],
    wire_backlash_dof: [Option<usize>; NUM_JOINTS],
    actuator_qpos: Vec<usize>,
    actuator_dof: Vec<usize>,
    actuator_wire: Vec<Option<usize>>,
    /// BAM M6 torque path; `None` keeps the XML position actuators.
    bam: Option<BamState>,
    /// The XML's own position gain per actuator (0.55 for this model). The
    /// daemon's kp=200 is a Dynamixel register value; the position path maps it
    /// to `base_gain * kp / 200`, not to kp directly.
    base_gain: Vec<f64>,
    held_trunk: [f64; 7],
    held_joints: [f64; NUM_JOINTS],
    kp: f64,
    torque_on: bool,
    released: bool,
    steps_per_tick: usize,
}

// The app's control loop owns this on one thread; the raw MuJoCo pointers are
// not shared. `Send` lets it be moved into that thread at startup.
unsafe impl Send for MujocoIo {}

impl MujocoIo {
    /// Load with the BAM M6 torque path (the real servo model).
    pub fn load(xml_path: impl AsRef<Path>) -> std::result::Result<Self, String> {
        Self::load_with_mode(xml_path, true)
    }

    /// Load with the XML position actuators (the official `duck-body` twin).
    pub fn load_position(xml_path: impl AsRef<Path>) -> std::result::Result<Self, String> {
        Self::load_with_mode(xml_path, false)
    }

    fn load_with_mode(
        xml_path: impl AsRef<Path>,
        use_bam: bool,
    ) -> std::result::Result<Self, String> {
        let path = CString::new(xml_path.as_ref().to_string_lossy().as_bytes())
            .map_err(|error| error.to_string())?;
        let mut error = [0 as c_char; 1024];
        let handle = unsafe { duck_mj_load(path.as_ptr(), error.as_mut_ptr(), error.len() as c_int) };
        if handle.is_null() {
            let message = unsafe { CStr::from_ptr(error.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(message);
        }
        // The walk policy was trained with 30 solver iterations and 50
        // line-search iterations (the MJCF default is 100/20); matching the
        // training solver is part of running the same duck.
        unsafe { duck_mj_set_solver(handle, 30, 50) };

        let nq = unsafe { duck_mj_nq(handle) } as usize;
        let nv = unsafe { duck_mj_nv(handle) } as usize;
        let nu = unsafe { duck_mj_nu(handle) } as usize;

        let trunk = CString::new(TRUNK_JOINT).expect("static string");
        let trunk_joint = unsafe { duck_mj_joint_id(handle, trunk.as_ptr()) };
        if trunk_joint < 0 {
            unsafe { duck_mj_free(handle) };
            return Err(format!("model has no joint {TRUNK_JOINT}"));
        }
        let trunk_qpos = unsafe { duck_mj_jnt_qposadr(handle, trunk_joint) } as usize;
        let trunk_dof = unsafe { duck_mj_jnt_dofadr(handle, trunk_joint) } as usize;

        let mut wire_actuator = [None; NUM_JOINTS];
        let mut wire_qpos = [None; NUM_JOINTS];
        let mut wire_dof = [None; NUM_JOINTS];
        let mut wire_backlash_qpos = [None; NUM_JOINTS];
        let mut wire_backlash_dof = [None; NUM_JOINTS];
        let mut actuator_qpos = vec![0usize; nu];

        for actuator in 0..nu {
            let joint = unsafe { duck_mj_actuator_trnid(handle, actuator as c_int, 0) };
            if joint >= 0 {
                actuator_qpos[actuator] = unsafe { duck_mj_jnt_qposadr(handle, joint) } as usize;
            }
        }
        let base_gain: Vec<f64> = (0..nu)
            .map(|actuator| unsafe { duck_mj_position_gain(handle, actuator as c_int) })
            .collect();

        let mut actuator_dof = vec![0usize; nu];
        let mut actuator_wire = vec![None; nu];
        for actuator in 0..nu {
            let joint = unsafe { duck_mj_actuator_trnid(handle, actuator as c_int, 0) };
            if joint >= 0 {
                actuator_dof[actuator] =
                    unsafe { duck_mj_jnt_dofadr(handle, joint) } as usize;
            }
        }
        for wire in 0..NUM_JOINTS {
            if let Some(actuator) = wire_actuator[wire] {
                actuator_wire[actuator] = Some(wire);
            }
        }

        for (wire, name) in WIRE_NAMES.iter().enumerate() {
            let cname = CString::new(*name).expect("static string");
            let actuator = unsafe { duck_mj_actuator_id(handle, cname.as_ptr()) };
            if actuator < 0 {
                continue;
            }
            let actuator = actuator as usize;
            let joint = unsafe { duck_mj_actuator_trnid(handle, actuator as c_int, 0) };
            if joint < 0 {
                continue;
            }
            wire_actuator[wire] = Some(actuator);
            wire_qpos[wire] = Some(unsafe { duck_mj_jnt_qposadr(handle, joint) } as usize);
            wire_dof[wire] = Some(unsafe { duck_mj_jnt_dofadr(handle, joint) } as usize);
            let backlash_name = format!("passive_{}_backlash", WIRE_NAMES[wire]);
            if let Ok(name) = std::ffi::CString::new(backlash_name) {
                let backlash = unsafe { duck_mj_joint_id(handle, name.as_ptr()) };
                if backlash >= 0 {
                    wire_backlash_qpos[wire] =
                        Some(unsafe { duck_mj_jnt_qposadr(handle, backlash) } as usize);
                    wire_backlash_dof[wire] =
                        Some(unsafe { duck_mj_jnt_dofadr(handle, backlash) } as usize);
                }
            }
        }

        let mut io = Self {
            handle,
            nq,
            nv,
            nu,
            trunk_qpos,
            trunk_dof,
            wire_actuator,
            wire_qpos,
            wire_dof,
            wire_backlash_qpos,
            wire_backlash_dof,
            actuator_qpos,
            actuator_dof,
            actuator_wire,
            bam: use_bam.then(|| BamState {
                model: BamModel::xl330_m6(),
                q_target: vec![0.0; nu],
                previous_torque: vec![0.0; nu],
            }),
            base_gain,
            held_trunk: [0.0; 7],
            held_joints: [0.0; NUM_JOINTS],
            kp: 200.0,
            torque_on: false,
            released: false,
            steps_per_tick: STEPS_PER_TICK,
        };
        unsafe { duck_mj_set_timestep(handle, 0.005) };
        io.configure_bam();
        io.place_home();
        Ok(io)
    }

    /// Turn the XML position actuators into torque motors and zero the joint
    /// friction, as `infer_policy.load_mujoco_with_bam` does.
    fn configure_bam(&mut self) {
        let Some(bam) = self.bam.as_ref() else {
            return;
        };
        let force_limit = bam.model.force_limit();
        let armature = bam.model.armature;
        let handle = self.handle;
        for actuator in 0..self.nu {
            let joint = unsafe { duck_mj_actuator_trnid(handle, actuator as c_int, 0) };
            if joint < 0 {
                continue;
            }
            unsafe {
                duck_mj_make_motor(handle, actuator as c_int);
                duck_mj_set_force_limit(handle, actuator as c_int, force_limit);
                duck_mj_zero_joint_friction(
                    handle,
                    joint,
                    armature,
                    -5.0e4,
                    -2.0e2,
                    0.99,
                    0.9999,
                    0.001,
                    0.5,
                    2.0,
                );
            }
        }
        unsafe { duck_mj_set_const(handle) };
    }

    fn place_home(&mut self) {
        unsafe { duck_mj_reset(self.handle) };
        {
            let qpos = unsafe { slice::from_raw_parts_mut(duck_mj_qpos(self.handle), self.nq) };
            qpos[self.trunk_qpos..self.trunk_qpos + 7]
                .copy_from_slice(&[0.0, 0.0, HOME_TRUNK_Z, 1.0, 0.0, 0.0, 0.0]);
            for wire in 0..NUM_JOINTS {
                if let Some(address) = self.wire_qpos[wire] {
                    qpos[address] = DEFAULT_POSITION[wire];
                }
            }
            self.held_trunk.copy_from_slice(&qpos[self.trunk_qpos..self.trunk_qpos + 7]);
            self.held_joints = [0.0; NUM_JOINTS];
            for wire in 0..NUM_JOINTS {
                if let Some(address) = self.wire_qpos[wire] {
                    self.held_joints[wire] = qpos[address];
                }
            }
        }
        unsafe { duck_mj_forward(self.handle) };
        if self.bam.is_some() {
            let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
            let q_target: Vec<f64> = (0..self.nu)
                .map(|actuator| {
                    let mut value = qpos[self.actuator_qpos[actuator]];
                    if let Some(wire) = self.actuator_wire[actuator] {
                        if let Some(address) = self.wire_backlash_qpos[wire] {
                            value += qpos[address];
                        }
                    }
                    value
                })
                .collect();
            if let Some(bam) = &mut self.bam {
                bam.q_target = q_target;
                bam.previous_torque.fill(0.0);
            }
        }
    }

    fn restore_hold(&mut self) {
        let qpos = unsafe { slice::from_raw_parts_mut(duck_mj_qpos(self.handle), self.nq) };
        qpos[self.trunk_qpos..self.trunk_qpos + 7].copy_from_slice(&self.held_trunk);
        for wire in 0..NUM_JOINTS {
            if let Some(address) = self.wire_qpos[wire] {
                qpos[address] = self.held_joints[wire];
            }
        }
        let qvel = unsafe { slice::from_raw_parts_mut(duck_mj_qvel(self.handle), self.nv) };
        qvel.fill(0.0);
    }

    fn apply_gain(&self) {
        if self.bam.is_some() {
            return;
        }
        let scale = if self.torque_on { self.kp / 200.0 } else { 0.0 };
        for actuator in 0..self.nu {
            let gain = self.base_gain[actuator] * scale;
            unsafe { duck_mj_set_position_gain(self.handle, actuator as c_int, gain) };
        }
    }

    fn quat_gravity(quat: [f64; 4]) -> [f64; 3] {
        let mut matrix = [0.0; 9];
        unsafe { duck_mj_quat2mat(matrix.as_mut_ptr(), quat.as_ptr()) };
        [-matrix[6], -matrix[7], -matrix[8]]
    }

    /// The trunk's world position, for rendering and tests.
    pub fn trunk(&self) -> [f64; 3] {
        let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
        [
            qpos[self.trunk_qpos],
            qpos[self.trunk_qpos + 1],
            qpos[self.trunk_qpos + 2],
        ]
    }

    pub fn trunk_quat(&self) -> [f64; 4] {
        let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
        [
            qpos[self.trunk_qpos + 3],
            qpos[self.trunk_qpos + 4],
            qpos[self.trunk_qpos + 5],
            qpos[self.trunk_qpos + 6],
        ]
    }

    pub fn gravity(&self) -> [f64; 3] {
        Self::quat_gravity(self.trunk_quat())
    }

    /// Raw qpos for the renderer: (trunk 7, then the 15 wire joints incl. mouth).
    pub fn render_qpos(&self) -> Vec<f64> {
        let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
        let mut out = Vec::with_capacity(7 + NUM_JOINTS);
        out.extend_from_slice(&qpos[self.trunk_qpos..self.trunk_qpos + 7]);
        for wire in 0..NUM_JOINTS {
            out.push(self.wire_qpos[wire].map_or(0.0, |address| qpos[address]));
        }
        out
    }

    pub fn body_count(&self) -> usize {
        (unsafe { duck_mj_nbody(self.handle) }) as usize
    }

    pub fn body_pose(&self, body: usize) -> ([f64; 3], [f64; 4]) {
        let count = self.body_count();
        if body >= count {
            return ([0.0; 3], [1.0, 0.0, 0.0, 0.0]);
        }
        let pos = unsafe { slice::from_raw_parts(duck_mj_xpos(self.handle), count * 3) };
        let quat = unsafe { slice::from_raw_parts(duck_mj_xquat(self.handle), count * 4) };
        (
            [
                pos[body * 3],
                pos[body * 3 + 1],
                pos[body * 3 + 2],
            ],
            [
                quat[body * 4],
                quat[body * 4 + 1],
                quat[body * 4 + 2],
                quat[body * 4 + 3],
            ],
        )
    }

    /// MuJoCo's version, for the provenance fingerprint.
    pub fn version() -> String {
        unsafe {
            let pointer = duck_mj_version();
            if pointer.is_null() {
                return "unknown".to_owned();
            }
            std::ffi::CStr::from_ptr(pointer)
                .to_string_lossy()
                .into_owned()
        }
    }

    /// The simulation state, for the time machine: MuJoCo's integration state
    /// plus the bookkeeping that lives on this side of the boundary (the BAM
    /// torque history, the hold pose, torque and release flags).
    /// A deep copy of this body: its own model (the BAM loop writes friction
    /// into it every substep, so a shared model would make branches race) and
    /// its own data at the same state.
    pub fn fork(&self) -> Option<Self> {
        let handle = unsafe { duck_mj_clone(self.handle) };
        if handle.is_null() {
            return None;
        }
        let mut copy = Self {
            handle,
            nq: self.nq,
            nv: self.nv,
            nu: self.nu,
            trunk_qpos: self.trunk_qpos,
            trunk_dof: self.trunk_dof,
            wire_actuator: self.wire_actuator,
            wire_qpos: self.wire_qpos,
            wire_dof: self.wire_dof,
            wire_backlash_qpos: self.wire_backlash_qpos,
            wire_backlash_dof: self.wire_backlash_dof,
            actuator_qpos: self.actuator_qpos.clone(),
            actuator_dof: self.actuator_dof.clone(),
            actuator_wire: self.actuator_wire.clone(),
            base_gain: self.base_gain.clone(),
            held_trunk: self.held_trunk,
            held_joints: self.held_joints,
            kp: self.kp,
            torque_on: self.torque_on,
            released: self.released,
            bam: self.bam.as_ref().map(|bam| BamState {
                model: bam.model.clone(),
                q_target: bam.q_target.clone(),
                previous_torque: bam.previous_torque.clone(),
            }),
            steps_per_tick: self.steps_per_tick,
        };
        Some(copy)
    }

    /// The raw handle, for the runtime's snapshot path. Borrowed for the call only.
    pub fn handle(&self) -> *mut DuckMj {
        self.handle
    }

    pub fn snapshot_state(&self) -> Vec<f64> {
        let size = unsafe { duck_mj_state_size(self.handle) } as usize;
        let mut state = vec![0.0; size];
        unsafe { duck_mj_state_get(self.handle, state.as_mut_ptr()) };
        // The scalar bookkeeping, appended so a restore is one contiguous copy.
        state.push(self.kp);
        state.push(if self.torque_on { 1.0 } else { 0.0 });
        state.push(if self.released { 1.0 } else { 0.0 });
        state.extend_from_slice(&self.held_trunk);
        state.extend_from_slice(&self.held_joints);
        if let Some(bam) = &self.bam {
            state.push(bam.q_target.len() as f64);
            state.extend_from_slice(&bam.q_target);
            state.extend_from_slice(&bam.previous_torque);
        } else {
            state.push(0.0);
        }
        state
    }

    /// Put back what [`Self::snapshot_state`] returned. A size mismatch is
    /// refused rather than half-applied.
    pub fn restore_state(&mut self, state: &[f64]) -> bool {
        let size = unsafe { duck_mj_state_size(self.handle) } as usize;
        let fixed = size + 3 + 7 + NUM_JOINTS;
        if state.len() < fixed {
            return false;
        }
        unsafe { duck_mj_state_set(self.handle, state.as_ptr()) };
        let mut cursor = size;
        self.kp = state[cursor];
        self.torque_on = state[cursor + 1] > 0.5;
        self.released = state[cursor + 2] > 0.5;
        cursor += 3;
        self.held_trunk.copy_from_slice(&state[cursor..cursor + 7]);
        cursor += 7;
        self.held_joints
            .copy_from_slice(&state[cursor..cursor + NUM_JOINTS]);
        cursor += NUM_JOINTS;
        let count = state[cursor] as usize;
        cursor += 1;
        if count > 0 {
            if state.len() < cursor + count * 2 {
                return false;
            }
            if let Some(bam) = &mut self.bam {
                bam.q_target.copy_from_slice(&state[cursor..cursor + count]);
                bam.previous_torque
                    .copy_from_slice(&state[cursor + count..cursor + count * 2]);
            }
        }
        true
    }

    pub fn geom_count(&self) -> usize {
        (unsafe { duck_mj_ngeom(self.handle) }) as usize
    }

    /// The mesh as a triangle soup of (px, py, pz, nx, ny, nz) per vertex.
    ///
    /// This is `mesh_vert`, which is the STL after the compiler's own mesh
    /// frame — MuJoCo re-centres and re-orients every mesh when it compiles
    /// the model. Rendering the raw STL vertices instead puts each part in
    /// the wrong place: the duck comes apart into a pile of servos.
    pub fn mesh_soup(&self, mesh: usize) -> Option<Vec<f32>> {
        let count = (unsafe { duck_mj_nmesh(self.handle) }) as usize;
        if mesh >= count {
            return None;
        }
        let vertadr = unsafe { *duck_mj_mesh_vertadr(self.handle).add(mesh) } as usize;
        let vertnum = unsafe { *duck_mj_mesh_vertnum(self.handle).add(mesh) } as usize;
        let faceadr = unsafe { *duck_mj_mesh_faceadr(self.handle).add(mesh) } as usize;
        let facenum = unsafe { *duck_mj_mesh_facenum(self.handle).add(mesh) } as usize;
        if vertnum == 0 || facenum == 0 {
            return None;
        }
        let verts = unsafe {
            slice::from_raw_parts(duck_mj_mesh_vert(self.handle).add(vertadr * 3), vertnum * 3)
        };
        // Face indices are relative to this mesh's own vertex block.
        let faces = unsafe {
            slice::from_raw_parts(duck_mj_mesh_face(self.handle).add(faceadr * 3), facenum * 3)
        };
        let mut soup = Vec::with_capacity(facenum * 18);
        for face in faces.chunks_exact(3) {
            let mut points = [[0f64; 3]; 3];
            for (corner, index) in face.iter().enumerate() {
                let index = *index as usize;
                if index >= vertnum {
                    return None;
                }
                for axis in 0..3 {
                    points[corner][axis] = verts[index * 3 + axis] as f64;
                }
            }
            let u = [
                points[1][0] - points[0][0],
                points[1][1] - points[0][1],
                points[1][2] - points[0][2],
            ];
            let v = [
                points[2][0] - points[0][0],
                points[2][1] - points[0][1],
                points[2][2] - points[0][2],
            ];
            let mut normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if length > 0.0 {
                for value in &mut normal {
                    *value /= length;
                }
            } else {
                normal = [0.0, 0.0, 1.0];
            }
            for point in points {
                soup.extend_from_slice(&[
                    point[0] as f32,
                    point[1] as f32,
                    point[2] as f32,
                    normal[0] as f32,
                    normal[1] as f32,
                    normal[2] as f32,
                ]);
            }
        }
        Some(soup)
    }

    pub fn geom_info(&self, geom: usize) -> Option<GeomInfo> {
        if geom >= self.geom_count() {
            return None;
        }
        let pos = unsafe { slice::from_raw_parts(duck_mj_geom_pos(self.handle), self.geom_count() * 3) };
        let quat =
            unsafe { slice::from_raw_parts(duck_mj_geom_quat(self.handle), self.geom_count() * 4) };
        // Resolve the material: geom_rgba alone is the default grey for
        // every geom that references a material.
        let mut rgba = [0f32; 4];
        unsafe { duck_mj_geom_colour(self.handle, geom as c_int, rgba.as_mut_ptr()) };
        Some(GeomInfo {
            body: unsafe { duck_mj_geom_bodyid(self.handle, geom as c_int) } as usize,
            geom_type: unsafe { duck_mj_geom_type(self.handle, geom as c_int) },
            mesh: unsafe { duck_mj_geom_meshid(self.handle, geom as c_int) },
            group: unsafe { duck_mj_geom_group(self.handle, geom as c_int) },
            pos: [pos[geom * 3], pos[geom * 3 + 1], pos[geom * 3 + 2]],
            quat: [
                quat[geom * 4],
                quat[geom * 4 + 1],
                quat[geom * 4 + 2],
                quat[geom * 4 + 3],
            ],
            rgba,
        })
    }

    pub fn mesh_name(&self, mesh: usize) -> Option<String> {
        let ptr = unsafe { duck_mj_mesh_name(self.handle, mesh as c_int) };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
        }
    }

    pub fn mesh_count(&self) -> usize {
        (unsafe { duck_mj_nmesh(self.handle) }) as usize
    }
}

impl Drop for MujocoIo {
    fn drop(&mut self) {
        unsafe { duck_mj_free(self.handle) };
    }
}

impl RobotIo for MujocoIo {
    fn read(&mut self) -> Result<Sensors> {
        let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
        let qvel = unsafe { slice::from_raw_parts(duck_mj_qvel(self.handle), self.nv) };
        let force = unsafe { slice::from_raw_parts(duck_mj_actuator_force(self.handle), self.nu) };

        let mut positions = [0.0; NUM_JOINTS];
        let mut velocities = [0.0; NUM_JOINTS];
        let mut currents = [0.0; NUM_JOINTS];
        for wire in 0..NUM_JOINTS {
            if let Some(actuator) = self.wire_actuator[wire] {
                // The encoder sits on the output side of the gear play, so it
                // reads servo + backlash. The training env's
                // BacklashEncoderBamActuator and joint_pos_rel_backlash use
                // exactly this sum; the motor-side angle alone is up to a
                // degree away from the pose the policy was trained to see.
                let servo = self.wire_qpos[wire].expect("actuator has qpos");
                let backlash = self.wire_backlash_qpos[wire];
                positions[wire] = qpos[servo] + backlash.map_or(0.0, |address| qpos[address]);
                let servo_dof = self.wire_dof[wire].expect("actuator has dof");
                let backlash_dof = self.wire_backlash_dof[wire];
                velocities[wire] =
                    qvel[servo_dof] + backlash_dof.map_or(0.0, |address| qvel[address]);
                currents[wire] = force[actuator].abs() * 100.0;
            }
        }

        let quat = [
            qpos[self.trunk_qpos + 3],
            qpos[self.trunk_qpos + 4],
            qpos[self.trunk_qpos + 5],
            qpos[self.trunk_qpos + 6],
        ];
        let gyro = [
            qvel[self.trunk_dof + 3],
            qvel[self.trunk_dof + 4],
            qvel[self.trunk_dof + 5],
        ];

        Ok(Sensors {
            positions,
            velocities,
            currents_ma: currents,
            imu: ImuData {
                gyro,
                gravity: Self::quat_gravity(quat),
                quat,
            },
        })
    }

    fn write(&mut self, targets: &JointTargets) -> Result<()> {
        if self.bam.is_some() {
            return self.write_bam_inner(targets);
        }
        {
            let ctrl = unsafe { slice::from_raw_parts_mut(duck_mj_ctrl(self.handle), self.nu) };
            if self.torque_on {
                for wire in 0..NUM_JOINTS {
                    if let Some(actuator) = self.wire_actuator[wire] {
                        ctrl[actuator] = targets.positions[wire];
                    }
                }
            } else {
                ctrl.fill(0.0);
            }
        }

        for _ in 0..self.steps_per_tick {
            unsafe { duck_mj_step(self.handle) };
            if !self.released {
                self.restore_hold();
                unsafe { duck_mj_forward(self.handle) };
            }
        }
        Ok(())
    }

    fn fork(&self) -> Option<Self> {
        MujocoIo::fork(self)
    }

    fn snapshot(&self) -> Option<Vec<f64>> {
        Some(self.snapshot_state())
    }

    fn restore(&mut self, state: &[f64]) -> bool {
        self.restore_state(state)
    }

    fn measures_velocity(&self) -> bool {
        true
    }

    fn has_mouth(&self) -> bool {
        self.wire_actuator[duck_control::model::MOUTH_INDEX].is_some()
    }

    fn trunk_position(&self) -> Option<[f64; 3]> {
        Some(self.trunk())
    }

    fn push(&mut self, velocity: [f64; 3]) -> Result<()> {        unsafe {
            duck_mj_set_trunk_velocity(
                self.handle,
                velocity[0],
                velocity[1],
                velocity[2],
            );
        }
        Ok(())
    }

    /// A hand holding the duck: a spring-damper toward `anchor + offset`.
    ///
    /// This is an external force (MuJoCo's `xfrc_applied`), not a joint target,
    /// so the servos still see only what the loop writes. A carry is a real
    /// manipulation — the pickup classifier reads the same gyro, joints and
    /// currents it would read if a person lifted the physical duck, which is
    /// what makes the pause testable rather than scripted.
    fn carry(&mut self, anchor: Option<[f64; 3]>, offset: [f64; 3]) {
        let Some(anchor) = anchor else {
            unsafe { duck_mj_set_trunk_force(self.handle, 0.0, 0.0, 0.0) };
            return;
        };
        let target = [
            anchor[0] + offset[0],
            anchor[1] + offset[1],
            (anchor[2] + offset[2]).max(0.045),
        ];
        let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
        let qvel = unsafe { slice::from_raw_parts(duck_mj_qvel(self.handle), self.nv) };
        let mut force = [0.0; 3];
        for axis in 0..3 {
            let position = qpos[self.trunk_qpos + axis];
            let velocity = qvel[self.trunk_dof + axis];
            force[axis] = CARRY_STIFFNESS * (target[axis] - position)
                - CARRY_DAMPING * velocity;
        }
        // A hand is strong but not a rigid weld; with the duck's 0.77 kg, 60 N is
        // already far past any acceleration a person would impart.
        let magnitude = force.iter().map(|value| value * value).sum::<f64>().sqrt();
        if magnitude > CARRY_MAX_FORCE {
            let scale = CARRY_MAX_FORCE / magnitude;
            for value in &mut force {
                *value *= scale;
            }
        }
        unsafe { duck_mj_set_trunk_force(self.handle, force[0], force[1], force[2]) };
    }

    fn measures_load(&self) -> bool {
        true
    }

    fn set_gain(&mut self, kp: u16) -> Result<()> {
        self.kp = kp as f64;
        if let Some(bam) = &mut self.bam {
            bam.model.kp = kp as f64;
        } else {
            self.apply_gain();
        }
        Ok(())
    }

    fn set_torque(&mut self, on: bool) -> Result<()> {
        if self.bam.is_some() {
            self.torque_on = on;
            if on {
                self.released = true;
                let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
                let q_target: Vec<f64> = (0..self.nu)
                    .map(|actuator| qpos[self.actuator_qpos[actuator]])
                    .collect();
                if let Some(bam) = &mut self.bam {
                    bam.q_target = q_target;
                    bam.previous_torque.fill(0.0);
                }
            } else {
                let ctrl =
                    unsafe { slice::from_raw_parts_mut(duck_mj_ctrl(self.handle), self.nu) };
                ctrl.fill(0.0);
            }
            return Ok(());
        }

        self.torque_on = on;
        if on {
            self.released = true;
            let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
            let ctrl = unsafe { slice::from_raw_parts_mut(duck_mj_ctrl(self.handle), self.nu) };
            for actuator in 0..self.nu {
                ctrl[actuator] = qpos[self.actuator_qpos[actuator]];
            }
        } else {
            let ctrl = unsafe { slice::from_raw_parts_mut(duck_mj_ctrl(self.handle), self.nu) };
            ctrl.fill(0.0);
        }
        self.apply_gain();
        Ok(())
    }

    fn reboot(&mut self, _id: u8) -> Result<()> {
        Ok(())
    }

    fn slow_sensors(&mut self) -> Result<SlowSensors> {
        // The servos see the pack through their own load: the BAM model already
        // computes that terminal voltage (`vin - drop x |torque|`, floored at
        // the M6's minimum), so report it rather than a nominal 7.4 V. A
        // walking duck reads a sagging pack, standing still does not — which is
        // what the real robot's battery percentage does.
        let volts = match &self.bam {
            Some(bam) => bam.model.vin_effective(&bam.previous_torque),
            None => 7.4,
        };
        // No thermal model exists in the training stack, so the case
        // temperature is ambient. Inventing a rise from current would be a
        // number nobody could calibrate against the real duck.
        Ok(SlowSensors {
            volts,
            temps_c: [25.0; NUM_JOINTS],
        })
    }
}

impl MujocoIo {
    /// The BAM torque path: voltage from the firmware P loop, DC-motor torque,
    /// and the M6 friction budget, once per 5 ms physics substep.
    fn write_bam_inner(&mut self, targets: &JointTargets) -> Result<()> {
        let mut bam = self.bam.take().expect("write_bam_inner implies BAM");
        for wire in 0..NUM_JOINTS {
            if let Some(actuator) = self.wire_actuator[wire] {
                bam.q_target[actuator] = targets.positions[wire];
            }
        }

        for _ in 0..self.steps_per_tick {
            let vin = bam.model.vin_effective(&bam.previous_torque);
            let mut torques = vec![0.0; self.nu];
            {
                let qpos = unsafe { slice::from_raw_parts(duck_mj_qpos(self.handle), self.nq) };
                let qvel = unsafe { slice::from_raw_parts(duck_mj_qvel(self.handle), self.nv) };
                for actuator in 0..self.nu {
                    // Same encoder semantics as the observation: position
                    // through the backlash play, velocity motor-side (the
                    // training stack leaves cmd.vel untouched).
                    let mut q = qpos[self.actuator_qpos[actuator]];
                    if let Some(wire) = self.actuator_wire[actuator] {
                        if let Some(address) = self.wire_backlash_qpos[wire] {
                            q += qpos[address];
                        }
                    }
                    let dq = qvel[self.actuator_dof[actuator]];
                    let volts = bam.model.voltage(bam.q_target[actuator], q, dq, vin);
                    let torque = if self.torque_on {
                        bam.model.torque(volts, dq)
                    } else {
                        0.0
                    };
                    torques[actuator] = torque;

                    let dof = self.actuator_dof[actuator] as c_int;
                    let external = unsafe { duck_mj_external_torque(self.handle, dof) };
                    let motor = unsafe { duck_mj_actuator_torque(self.handle, dof) };
                    let (frictionloss, damping) = bam.model.frictions(motor, external, dq);
                    unsafe {
                        duck_mj_set_ctrl(self.handle, actuator as c_int, torque);
                        duck_mj_set_dof_friction(self.handle, dof, frictionloss, damping);
                    }
                }
            }

            unsafe { duck_mj_step(self.handle) };
            if !self.released {
                self.restore_hold();
                unsafe { duck_mj_forward(self.handle) };
            }
            bam.previous_torque.copy_from_slice(&torques);
        }

        self.bam = Some(bam);
        Ok(())
    }
}

#[cfg(all(test, feature = "mujoco"))]
mod tests {
    use super::*;

    /// The roller scene must load through the same mapping the walking one
    /// uses: same trunk freejoint, the same 14 actuators by name, and passive
    /// wheel joints that are simply not in the wire table.
    #[test]
    fn the_roller_scene_loads_with_the_same_wire_mapping() {
        let scene = std::path::Path::new(
            r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_rollers.xml",
        );
        if !scene.exists() {
            return;
        }
        let io = MujocoIo::load(scene).expect("the roller scene loads");
        assert!(io.body_count() > 0);
        // Every leg, neck and head servo is present. The roller model has no
        // mouth joint, exactly like the walking one, so a beak intent is
        // refused there too rather than accepted and dropped.
        assert!(!io.has_mouth());
        assert_eq!(io.nu, 14, "the same fourteen servos as the walking model");
    }
}

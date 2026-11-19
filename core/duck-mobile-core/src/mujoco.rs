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

use crate::bam::{BamModel, current_ma};

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
    fn duck_mj_ncon(handle: *const DuckMj) -> c_int;
    fn duck_mj_contact_geoms(handle: *const DuckMj) -> *const c_int;
    fn duck_mj_geom_name(handle: *const DuckMj, geom: c_int) -> *const c_char;
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
    fn duck_mj_set_dof_friction(
        handle: *mut DuckMj,
        dof: c_int,
        frictionloss: c_double,
        damping: c_double,
    );
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
/// `duck_mj_contact_geoms`' static buffer holds 512 `(geom1, geom2)` pairs
/// (`pairs[1024]` ints in `native/duck_mj_shim.cpp`). A step can report more
/// contacts than that; `ncon` is clamped to this at both sides of the seam.
#[cfg(test)]
const CONTACT_PAIR_LIMIT: usize = 512;

/// The shell's scan output, in its writable staging directory.
pub const OBSTACLES_FILE: &str = "obstacles.xml";
/// The compiled scene the merge writes beside it; removed after loading.
const MERGED_SCENE_FILE: &str = "duck_scene_merged.xml";

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
    /// The motor's torque constant, in newton-metres per ampere. A hardware
    /// number, not a modelling choice: it turns an actuator's torque back
    /// into the current the servo's `present_current` register would report.
    motor_kt: f64,
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
    /// The scene's own floor height at load (the training scene's 0). The
    /// AR floor is addressed as an offset from this, never as an absolute
    /// number a shell happened to measure.
    floor_base_z: f64,
}

// The app's control loop owns this on one thread; the raw MuJoCo pointers are
// not shared. `Send` lets it be moved into that thread at startup.
unsafe impl Send for MujocoIo {}

impl MujocoIo {
    /// The AR mode's floor hook (SENSORS_PLAN S2): write the plane geom's
    /// z directly in mjModel and re-run the solver's forward pass. The
    /// duck's senses do not change - the WORLD does, which is what makes
    /// this the honest AR move rather than a possession trick.
    /// Returns the geom it moved, for the route to report.
    ///
    /// This is the **absolute** model hook (tests and diagnostics). The
    /// route speaks [`Self::set_floor_delta`]: an offset from the scene's
    /// own floor, because a shell's measured plane height is not a world z.
    pub fn set_floor_z(&mut self, z: f64) -> Option<usize> {
        let (geom, _) = self.plane()?;
        let count = self.geom_count();
        let pos = unsafe { slice::from_raw_parts_mut(duck_mj_geom_pos(self.handle), count * 3) };
        pos[geom * 3 + 2] = z;
        unsafe { duck_mj_forward(self.handle) };
        Some(geom)
    }

    /// The AR floor as the route defines it: `offset_m` is the walkable
    /// plane's elevation over the scene's default floor (`0` = unchanged,
    /// the training scene's plane). Both the plane **and the duck** move by
    /// the same amount, so the duck keeps standing on its floor instead of
    /// being buried in it; this is a world transform, not a sense.
    ///
    /// Returns the plane geom, or `None` when the scene has no ground plane.
    pub fn set_floor_delta(&mut self, offset_m: f64) -> Option<usize> {
        let (geom, current) = self.plane()?;
        let target = self.floor_base_z + offset_m;
        let step = target - current;
        if step != 0.0 {
            let count = self.geom_count();
            let pos =
                unsafe { slice::from_raw_parts_mut(duck_mj_geom_pos(self.handle), count * 3) };
            pos[geom * 3 + 2] = target;
            // The duck stands on this plane: lift/lower it with the world.
            // The held pose is the limp body's restore target, so it moves
            // too, or the next `write` would snap the duck back under the
            // floor.
            let qpos = unsafe { slice::from_raw_parts_mut(duck_mj_qpos(self.handle), self.nq) };
            qpos[self.trunk_qpos + 2] += step;
            self.held_trunk[2] += step;
            unsafe { duck_mj_forward(self.handle) };
        }
        Some(geom)
    }

    /// The first plane geom and its current world z, if the scene has one.
    fn plane(&self) -> Option<(usize, f64)> {
        let count = self.geom_count();
        let pos = unsafe { slice::from_raw_parts(duck_mj_geom_pos(self.handle), count * 3) };
        (0..count).find_map(|geom| {
            (unsafe { duck_mj_geom_type(self.handle, geom as c_int) } == 0)
                .then(|| (geom, pos[geom * 3 + 2]))
        })
    }

    /// Move the trunk's freejoint to `position` (world x, y, z) and re-run
    /// the forward pass. A test-only sweep hook: the loop itself never
    /// teleports the duck, but a physics experiment may place it.
    #[cfg(test)]
    pub fn set_trunk_position(&mut self, position: [f64; 3]) {
        let qpos = unsafe { slice::from_raw_parts_mut(duck_mj_qpos(self.handle), self.nq) };
        qpos[self.trunk_qpos..self.trunk_qpos + 3].copy_from_slice(&position);
        let qvel = unsafe { slice::from_raw_parts_mut(duck_mj_qvel(self.handle), self.nv) };
        qvel.fill(0.0);
        unsafe { duck_mj_forward(self.handle) };
    }

    /// One 5 ms physics substep, without the control loop's hold. Test-only:
    /// after [`Self::set_trunk_position`] the contacts are what the sweep
    /// reads.
    #[cfg(test)]
    pub fn step_once(&mut self) {
        unsafe { duck_mj_step(self.handle) };
    }

    /// Whether any geom named `depth_obstacle_…` is in contact with one of
    /// the duck's own bodies. The contact buffer is MuJoCo's, after a step.
    #[cfg(test)]
    pub fn scanned_geom_touching(&self) -> bool {
        for (left, right) in self.contacts() {
            if left.starts_with("depth_obstacle_") || right.starts_with("depth_obstacle_") {
                return true;
            }
        }
        false
    }

    /// The contact list as `(geom_name, other_geom_name)` pairs. Test-only.
    #[cfg(test)]
    fn contacts(&self) -> Vec<(String, String)> {
        // The shim's buffer holds 512 pairs and clamps `ncon` to match; the
        // min here keeps the slice honest even if a future shim forgets.
        let count = (unsafe { duck_mj_ncon(self.handle) } as usize).min(CONTACT_PAIR_LIMIT);
        if count == 0 {
            return Vec::new();
        }
        let pairs = unsafe { slice::from_raw_parts(duck_mj_contact_geoms(self.handle), count * 2) };
        (0..count)
            .map(|contact| {
                let name = |geom: c_int| -> String {
                    if geom < 0 {
                        String::new()
                    } else {
                        let pointer = unsafe { duck_mj_geom_name(self.handle, geom) };
                        if pointer.is_null() {
                            String::new()
                        } else {
                            unsafe { CStr::from_ptr(pointer) }
                                .to_string_lossy()
                                .into_owned()
                        }
                    }
                };
                (name(pairs[contact * 2]), name(pairs[contact * 2 + 1]))
            })
            .collect()
    }

    /// Load with the BAM M6 torque path (the real servo model).
    pub fn load(xml_path: impl AsRef<Path>) -> std::result::Result<Self, String> {
        Self::load_compiled(xml_path, true)
    }

    /// Load with the XML position actuators (the official `duck-body` twin).
    pub fn load_position(xml_path: impl AsRef<Path>) -> std::result::Result<Self, String> {
        Self::load_compiled(xml_path, false)
    }

    /// Load a scene with the room's scanned furniture merged in.
    ///
    /// `scene_dir` is the shell's writable staging directory; a file named
    /// [`OBSTACLES_FILE`] there is the AR depth scan's output — a complete
    /// MJCF document whose `<worldbody>` holds static box geoms at absolute
    /// world positions (see [`merged_scene_xml`]). The geoms are inserted
    /// into the scene's worldbody before compiling, so the duck and the
    /// furniture share one physics world. No file means exactly
    /// [`Self::load`] — the feature is off, not approximated.
    ///
    /// A scan that cannot be read, parsed, written, or compiled is **dropped
    /// with a warning** and the plain scene loads: a corrupt AR scan must not
    /// stop the duck from starting. The shell's own tripwire can report the
    /// scan; the core's job is to keep the animal alive.
    ///
    /// The merged XML is written to the staging directory (or the system
    /// temp dir if staging is read-only) because MuJoCo 3.10's `mj_loadXML`
    /// takes a path, not a string; the shim is deliberately untouched. The
    /// original scene is `<include>`d by absolute path and the asset
    /// directories are pinned absolute, so the merged file's own location
    /// does not affect `meshdir` resolution.
    pub fn load_with_obstacles(
        scene: impl AsRef<Path>,
        scene_dir: impl AsRef<Path>,
    ) -> std::result::Result<Self, String> {
        // The temporary directory is injectable so the write-failure fallback
        // is testable without mutating process-wide environment state; the
        // product always uses the system temp dir.
        Self::load_with_obstacles_using(scene.as_ref(), scene_dir.as_ref(), &std::env::temp_dir())
    }

    fn load_with_obstacles_using(
        scene: &Path,
        scene_dir: &Path,
        temporary: &Path,
    ) -> std::result::Result<Self, String> {
        let obstacles = scene_dir.join(OBSTACLES_FILE);
        if !obstacles.is_file() {
            return Self::load(scene);
        }
        let merged = std::fs::read_to_string(&obstacles)
            .map_err(|error| format!("reading {}: {error}", obstacles.display()))
            .and_then(|fragment| {
                merged_scene_xml(scene, &fragment)
                    .map_err(|error| format!("{}: {error}", obstacles.display()))
            });
        let merged = match merged {
            Ok(xml) => xml,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the AR scan cannot be merged; loading the room without it"
                );
                return Self::load(scene);
            }
        };
        let merged_path = match write_merged_scene(scene_dir, temporary, &merged) {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    %error,
                    scan = %obstacles.display(),
                    "cannot write the merged scene; loading the room without the scan"
                );
                return Self::load(scene);
            }
        };
        let result = Self::load_compiled(&merged_path, true);
        // `mj_loadXML` has read the file into the model by now; the compiled
        // copy is the truth.
        let _ = std::fs::remove_file(&merged_path);
        match result {
            Ok(io) => Ok(io),
            Err(error) => {
                tracing::warn!(
                    %error,
                    scan = %obstacles.display(),
                    "the scanned room did not compile; loading it without the scan"
                );
                // The scene itself is the one thing that must load. If this
                // fails too, its error — not the scan's — is the truth.
                Self::load(scene)
            }
        }
    }

    fn load_compiled(
        xml_path: impl AsRef<Path>,
        use_bam: bool,
    ) -> std::result::Result<Self, String> {
        let path = CString::new(xml_path.as_ref().to_string_lossy().as_bytes())
            .map_err(|error| error.to_string())?;
        let mut error = [0 as c_char; 1024];
        let handle =
            unsafe { duck_mj_load(path.as_ptr(), error.as_mut_ptr(), error.len() as c_int) };
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
                actuator_dof[actuator] = unsafe { duck_mj_jnt_dofadr(handle, joint) } as usize;
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

        let model = BamModel::xl330_m6();
        // The scene's own floor, for the route's offset semantics. A body
        // with no plane has no floor to offset; zero is never used because
        // `set_floor_delta` refuses without a plane.
        let floor_base_z = {
            let count = unsafe { duck_mj_ngeom(handle) } as usize;
            let pos = unsafe { slice::from_raw_parts(duck_mj_geom_pos(handle), count * 3) };
            (0..count)
                .find(|geom| unsafe { duck_mj_geom_type(handle, *geom as c_int) } == 0)
                .map_or(0.0, |geom| pos[geom * 3 + 2])
        };
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
                model: model.clone(),
                q_target: vec![0.0; nu],
                previous_torque: vec![0.0; nu],
            }),
            motor_kt: model.kt,
            base_gain,
            held_trunk: [0.0; 7],
            held_joints: [0.0; NUM_JOINTS],
            kp: 200.0,
            torque_on: false,
            released: false,
            steps_per_tick: STEPS_PER_TICK,
            floor_base_z,
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
                    handle, joint, armature, -5.0e4, -2.0e2, 0.99, 0.9999, 0.001, 0.5, 2.0,
                );
            }
        }
        unsafe { duck_mj_set_const(handle) };
    }

    fn place_home(&mut self) {
        unsafe { duck_mj_reset(self.handle) };
        {
            let qpos = unsafe { slice::from_raw_parts_mut(duck_mj_qpos(self.handle), self.nq) };
            qpos[self.trunk_qpos..self.trunk_qpos + 7].copy_from_slice(&[
                0.0,
                0.0,
                HOME_TRUNK_Z,
                1.0,
                0.0,
                0.0,
                0.0,
            ]);
            for wire in 0..NUM_JOINTS {
                if let Some(address) = self.wire_qpos[wire] {
                    qpos[address] = DEFAULT_POSITION[wire];
                }
            }
            self.held_trunk
                .copy_from_slice(&qpos[self.trunk_qpos..self.trunk_qpos + 7]);
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
            [pos[body * 3], pos[body * 3 + 1], pos[body * 3 + 2]],
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
            motor_kt: self.motor_kt,
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
            floor_base_z: self.floor_base_z,
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
            let length =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
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
        let pos =
            unsafe { slice::from_raw_parts(duck_mj_geom_pos(self.handle), self.geom_count() * 3) };
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
            Some(
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned(),
            )
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

/// Merge the AR scan document into the scene as a new MJCF root.
///
/// The scan contract (both shells write this; the fixtures in `tests/` are
/// the reference):
///
/// ```xml
/// <mujoco model="duck-depth-obstacles">
///   <worldbody>
///     <geom name="depth_obstacle_0" type="box" size="0.10 0.20 0.05"
///           pos="0.4 0 0.05" condim="3"/>
///   </worldbody>
/// </mujoco>
/// ```
///
/// One to three static `<geom type="box">` entries, `size` half-extents in
/// metres, `pos` the box centre in absolute world coordinates. A bare
/// `<geom>`/`<body>` list with no wrapper is accepted too. The outer
/// `<mujoco>`/`<worldbody>` is **stripped**: only the worldbody children are
/// copied into the scene's world, so a nested document never reaches
/// `mj_loadXML` and the scan cannot smuggle in assets, actuators or
/// sensors. A body-less geom under `worldbody` is static by construction;
/// default `contype`/`conaffinity` means it collides with the robot's
/// `collision` geoms exactly as the floor does.
///
/// The merge includes the original scene by absolute path and repeats the
/// scene's asset directories as absolute paths in a trailing `<compiler>`.
/// That trailing element is deliberate: MuJoCo parses every `compiler` in
/// document order, and the included robot's `meshdir="assets"` would
/// otherwise resolve against *this* file's directory rather than the
/// scene's.
fn merged_scene_xml(scene: &Path, fragment: &str) -> std::result::Result<String, String> {
    let body = obstacle_world_body(fragment)
        .ok_or_else(|| "the scan has no worldbody bodies to merge".to_owned())?;
    let scene_path = absolute_path(scene)?;
    let scene_dir = scene_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", scene_path.display()))?;
    let scene_xml = xml_escape(&slash_path(&scene_path));
    let dir_xml = xml_escape(&slash_path(scene_dir));
    Ok(format!(
        "<mujoco model=\"duck-scene-with-obstacles\">\n  \
         <include file=\"{scene_xml}\"/>\n  \
         <worldbody>\n{body}\n  </worldbody>\n  \
         <compiler assetdir=\"{dir_xml}\" meshdir=\"{dir_xml}/assets\" texturedir=\"{dir_xml}\"/>\n\
         </mujoco>\n"
    ))
}

/// The worldbody children inside a scan file, with any outer
/// `<mujoco>`/`<worldbody>` wrapper and XML declaration stripped.
///
/// Comments are skipped whole, before anything else looks for a tag: a
/// scan may carry a license or provenance header, and searching for
/// `<worldbody` inside a comment would find the merger's own words. A
/// comment that never closes refuses the scan: a truncated file's text is
/// not a document the author wrote, and a `<worldbody>` inside the unclosed
/// comment must not become the scan's wrapper.
fn obstacle_world_body(fragment: &str) -> Option<String> {
    let stripped = strip_comments(fragment.trim())?;
    if stripped.is_empty() {
        return None;
    }
    if let Some(open) = stripped.find("<worldbody") {
        // An unterminated close tag (a truncated scan) leaves no
        // `</worldbody>`: pass the whole document through, and let the XML
        // parse say loudly what is wrong instead of merging a fragment the
        // author never wrote.
        let Some(close) = stripped.rfind("</worldbody>") else {
            return Some(stripped);
        };
        let after_open = open + stripped[open..].find('>')? + 1;
        if close < after_open {
            return Some(stripped);
        }
        let body = stripped[after_open..close].trim();
        (!body.is_empty()).then(|| body.to_owned())
    } else {
        // No worldbody: either a bare geom list (fine as-is) or a document
        // whose wrapper holds nothing placeable.
        if stripped.starts_with("<?xml") {
            let after = stripped.find("?>")? + 2;
            let rest = stripped[after..].trim();
            if rest.starts_with("<mujoco") {
                return None;
            }
            (!rest.is_empty()).then(|| rest.to_owned())
        } else if stripped.starts_with("<mujoco") {
            None
        } else {
            Some(stripped.to_owned())
        }
    }
}

/// Remove `<!-- … -->` comments, including multi-line ones. `None` when a
/// comment never closes: the scan is truncated, and keeping its text would
/// let tags inside the comment masquerade as the document's own. The caller
/// refuses the whole scan rather than merge a fragment nobody wrote.
fn strip_comments(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<!--") {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find("-->") else {
            return None;
        };
        rest = &rest[open + close + 3..];
    }
    out.push_str(rest);
    Some(out)
}

/// The scene as an absolute path: MuJoCo is handed the merged file from a
/// different directory, and `<include>` must not depend on the process CWD.
fn absolute_path(path: &Path) -> std::result::Result<std::path::PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|error| format!("current dir: {error}"))?
            .join(path))
    }
}

/// XML attribute text. Windows separators become forward slashes so the
/// file works the same on every target.
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Write the merged scene where MuJoCo can read it. Staging first (the
/// directory the shell already writes to); the temp directory is the
/// fallback for a read-only bundle. Absolute paths inside the XML make the
/// second location behave identically to the first.
fn write_merged_scene(
    staging: &Path,
    temporary_dir: &Path,
    xml: &str,
) -> std::result::Result<std::path::PathBuf, String> {
    let staged = staging.join(MERGED_SCENE_FILE);
    match std::fs::write(&staged, xml) {
        Ok(()) => return Ok(staged),
        Err(error) => {
            tracing::warn!(
                path = %staged.display(),
                %error,
                "cannot write the merged scene beside the scan; using temp"
            );
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let temporary = temporary_dir.join(format!(
        "duck_scene_merged_{}_{stamp}.xml",
        std::process::id()
    ));
    std::fs::write(&temporary, xml)
        .map_err(|error| format!("writing {}: {error}", temporary.display()))?;
    Ok(temporary)
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
                // The bus reads each servo's `present_current` register, and
                // that is what `currents_ma` means downstream: upstream's
                // pickup classifier divides it by 1000 to get amps. The
                // actuator's force is a torque, so it goes through the
                // motor's torque constant first: I = τ / kt. Reading the
                // torque as milliamperes directly (it used to be torque ×
                // 100) made the current feature about 27 times too small,
                // and a duck held in a hand was never seen as held.
                currents[wire] = current_ma(force[actuator], self.motor_kt);
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

    fn push(&mut self, velocity: [f64; 3]) -> Result<()> {
        unsafe {
            duck_mj_set_trunk_velocity(self.handle, velocity[0], velocity[1], velocity[2]);
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
            force[axis] = CARRY_STIFFNESS * (target[axis] - position) - CARRY_DAMPING * velocity;
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
                let ctrl = unsafe { slice::from_raw_parts_mut(duck_mj_ctrl(self.handle), self.nu) };
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

    /// The scan fixture that ships with this repository, staged the way the
    /// shell does: as `obstacles.xml` in a writable directory.
    fn staging_with(fixture: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("duck-obstacles-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("staging directory");
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        std::fs::copy(&fixture, dir.join(OBSTACLES_FILE)).expect("stage the scan");
        dir
    }

    /// The one-box scan in the shells' full-document shape.
    fn staged_obstacles() -> std::path::PathBuf {
        staging_with("obstacles.xml")
    }

    // ── scan-file extraction (obstacle_world_body + strip_comments) ────────

    /// A scan whose comments mention the merger's own words must still work:
    /// the extraction reads the real geoms, never the comment's text.
    #[test]
    fn a_scan_with_comments_about_worldbody_extracts_the_real_geoms() {
        let scan = r#"<!-- provenance: scanned with the <worldbody> filter; see </worldbody> -->
<mujoco model="duck-depth-obstacles">
  <!-- inner note -->
  <worldbody>
    <geom name="depth_obstacle_0" type="box" size="0.10 0.20 0.05" pos="0.4 0 0.05"/>
  </worldbody>
</mujoco>"#;
        let body = obstacle_world_body(scan).expect("the real geoms survive the comments");
        assert!(body.contains("depth_obstacle_0"), "{body}");
        assert!(
            !body.contains("worldbody"),
            "the comment's words must not leak into the world: {body}"
        );
        assert!(!body.contains("provenance"), "{body}");
        assert!(!body.contains("inner note"), "{body}");
    }

    /// Multi-line comments, an XML declaration and the wrapper in one file,
    /// the worst real-world shape the shells produce.
    #[test]
    fn a_scan_with_declaration_comments_and_wrapper_extracts_the_geoms() {
        let scan = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!--\n  multi-line header\n  mentioning <worldbody> and </worldbody> on purpose\n-->\n",
            "<mujoco model=\"duck-depth-obstacles\">\n",
            "  <worldbody>\n",
            "    <geom name=\"depth_obstacle_0\" type=\"box\" size=\"0.1 0.1 0.05\" pos=\"0.5 0 0.05\"/>\n",
            "    <geom name=\"depth_obstacle_1\" type=\"box\" size=\"0.1 0.1 0.05\" pos=\"0.8 0 0.05\"/>\n",
            "  </worldbody>\n",
            "</mujoco>",
        );
        let body = obstacle_world_body(scan).expect("the wrapper opens the worldbody");
        assert!(body.contains("depth_obstacle_0"), "{body}");
        assert!(body.contains("depth_obstacle_1"), "{body}");
        assert!(!body.contains("multi-line header"), "{body}");
    }

    /// A bare geom list with no wrapper at all is accepted as-is.
    #[test]
    fn a_bare_geom_list_is_accepted_as_is() {
        let scan =
            r#"<geom name="depth_obstacle_0" type="box" size="0.1 0.1 0.05" pos="0.4 0 0.05"/>"#;
        let body = obstacle_world_body(scan).expect("a bare list is placeable");
        assert_eq!(body, scan.trim());
    }

    /// A wrapper without a worldbody has nothing to place.
    #[test]
    fn a_wrapper_without_a_worldbody_has_nothing_to_place() {
        assert_eq!(obstacle_world_body("<mujoco><asset/></mujoco>"), None);
        assert_eq!(
            obstacle_world_body("<?xml version=\"1.0\"?><mujoco/>"),
            None
        );
        assert_eq!(obstacle_world_body("   "), None);
    }

    /// An unterminated comment means a truncated scan. Cutting at the comment's
    /// open and keeping its text would let `<worldbody>` *inside* the comment
    /// masquerade as the scan's real wrapper; the extraction refuses the file
    /// instead, and the load path keeps the plain room. QA's sample is the
    /// first document below.
    #[test]
    fn an_unterminated_comment_rejects_the_scan() {
        let truncated = concat!(
            "<!-- truncated\n",
            "<mujoco><worldbody><geom name=\"depth_obstacle_0\" type=\"box\" ",
            "size=\"0.1 0.1 0.05\"/></worldbody></mujoco>",
        );
        assert_eq!(
            obstacle_world_body(truncated),
            None,
            "an unclosed comment is not a placeable scan"
        );

        let scan = concat!(
            "<worldbody><geom name=\"a\" type=\"box\" size=\"0.1 0.1 0.05\"/>\n",
            "<!-- this comment never ends",
        );
        assert_eq!(
            obstacle_world_body(scan),
            None,
            "the real wrapper is not trusted next to an unclosed comment"
        );

        // The whole load path answers the refusal honestly: the plain room.
        let scene = walking_scene();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "duck-unterminated-scan-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("staging");
        std::fs::write(dir.join(OBSTACLES_FILE), scan).expect("write scan");
        let plain = MujocoIo::load(&scene).expect("plain room");
        let body =
            MujocoIo::load_with_obstacles(&scene, &dir).expect("the scan falls back to plain");
        assert_eq!(
            body.geom_count(),
            plain.geom_count(),
            "a broken scan contributes nothing"
        );
    }

    /// The official walking scene, or the CI-required failure when the
    /// fetched upstream checkout is missing.
    fn walking_scene() -> std::path::PathBuf {
        crate::fixtures::scene_backlash().expect("the official training scene (fetch upstream)")
    }

    /// The scan adds world geometry and nothing else: one body, one geom,
    /// where the scan put it. The 61-D observation layout is a separate
    /// mechanism and is untouched by a static body with no joint.
    #[test]
    fn the_scan_adds_the_staged_box_to_the_world() {
        let scene = walking_scene();
        let staging = staged_obstacles();
        let plain = MujocoIo::load(&scene).expect("the plain scene loads");
        let scanned = MujocoIo::load_with_obstacles(&scene, &staging)
            .expect("the scene with furniture loads");

        assert_eq!(
            scanned.geom_count(),
            plain.geom_count() + 1,
            "one scanned box is one new geom"
        );
        assert_eq!(
            scanned.body_count(),
            plain.body_count(),
            "a scan geom is attached to the world, so no body is added"
        );
        assert_eq!(scanned.nq, plain.nq, "a static scan adds no qpos");
        assert_eq!(scanned.nu, plain.nu, "and no actuator");

        // MuJoCo's mjtGeom: box = 6. The scan's absolute centre is exactly
        // where the fixture says.
        let boxed = (0..scanned.geom_count()).any(|geom| {
            scanned.geom_info(geom).is_some_and(|info| {
                info.geom_type == 6
                    && (info.pos[0] - 0.4).abs() < 1e-9
                    && info.pos[1].abs() < 1e-9
                    && (info.pos[2] - 0.05).abs() < 1e-9
            })
        });
        assert!(boxed, "the scanned box is not at its world position");

        // No scan file at all is the old behaviour, not an empty merge.
        let empty = staging.join("no-such-directory");
        std::fs::create_dir_all(&empty).expect("empty staging");
        let unchanged =
            MujocoIo::load_with_obstacles(&scene, &empty).expect("a missing scan is plain load");
        assert_eq!(unchanged.geom_count(), plain.geom_count());
        assert_eq!(unchanged.body_count(), plain.body_count());
    }

    /// The shells cap a scan at three boxes; all three worldbody geoms must
    /// land, not just the first.
    #[test]
    fn a_three_box_scan_adds_all_three_geoms() {
        let scene = walking_scene();
        let staging = staging_with("obstacles_three.xml");
        let plain = MujocoIo::load(&scene).expect("the plain scene loads");
        let scanned =
            MujocoIo::load_with_obstacles(&scene, &staging).expect("the three-box scan loads");
        assert_eq!(
            scanned.geom_count(),
            plain.geom_count() + 3,
            "every scanned box is a geom"
        );
        assert_eq!(scanned.body_count(), plain.body_count());
        let boxes = (0..scanned.geom_count())
            .filter_map(|geom| scanned.geom_info(geom))
            .filter(|info| info.geom_type == 6)
            .count();
        assert_eq!(
            boxes, 3,
            "the three boxes are real box geoms, not one plus junk"
        );
    }

    /// A scan that cannot be read or placed is dropped with a warning: the
    /// plain room loads. A corrupt AR file must not stop the duck.
    #[test]
    fn a_corrupt_scan_falls_back_to_the_plain_room() {
        let scene = walking_scene();
        let plain = MujocoIo::load(&scene).expect("the plain scene loads");

        for (name, contents) in [
            ("no-worldbody", "<mujoco><asset/></mujoco>"),
            ("truncated", "<mujoco><worldbody><geom type=\"box\" size=\""),
            ("not-xml", "this is not a scan\n"),
        ] {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir().join(format!(
                "duck-bad-scan-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("staging");
            std::fs::write(dir.join(OBSTACLES_FILE), contents).expect("write scan");
            let body = MujocoIo::load_with_obstacles(&scene, &dir)
                .unwrap_or_else(|error| panic!("{name} must not brick the duck: {error}"));
            assert_eq!(
                body.geom_count(),
                plain.geom_count(),
                "{name}: the scan contributed nothing"
            );
            assert_eq!(body.body_count(), plain.body_count(), "{name}");
        }
    }

    /// A merged scene that cannot be *written* is a scan that cannot be
    /// merged: the plain room loads, exactly as for a scan that cannot be
    /// read or parsed. Both write locations are made to fail here — the
    /// staging path is a directory and the injected temp root is a file — so
    /// the failure is deterministic on every platform without touching
    /// process-wide environment state.
    #[test]
    fn an_unwritable_merged_scene_falls_back_to_the_plain_room() {
        let scene = walking_scene();
        let plain = MujocoIo::load(&scene).expect("the plain scene loads");
        let staging = staged_obstacles();
        std::fs::create_dir_all(staging.join(MERGED_SCENE_FILE))
            .expect("a directory blocks the staged file path");
        let blocked_temp = staging.join("not-a-directory");
        std::fs::write(&blocked_temp, b"a file, not a temp dir").expect("block the temp root");

        let body = MujocoIo::load_with_obstacles_using(&scene, &staging, &blocked_temp)
            .expect("an unwritable merge must not brick the duck");
        assert_eq!(
            body.geom_count(),
            plain.geom_count(),
            "a scan that cannot be written contributes nothing"
        );
        assert_eq!(body.body_count(), plain.body_count());
    }

    /// The walking loop, against the real MuJoCo body and the official
    /// velocity policy, for a fixed six simulated seconds. Returns whether
    /// the safety layer latched a fall.
    fn walk_into_the_room(scene: &Path, staging: Option<&Path>) -> (bool, f64) {
        use duck_control::policy::{Policy, PolicyPaths};
        use duck_control::safety::SafetyConfig;

        let body = match staging {
            Some(dir) => MujocoIo::load_with_obstacles(scene, dir).expect("body with scan"),
            None => MujocoIo::load(scene).expect("plain body"),
        };
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let policy = Policy::load(
            &PolicyPaths {
                walk: dir.join("alpha_walking.onnx"),
                stand: Some(dir.join("alpha_stand.onnx")),
                sitstand: None,
                ground_pick: None,
                skills: Vec::new(),
            },
            0.05,
        )
        .expect("the official velocity policy");
        let mut runtime = crate::runtime::DuckRuntime::new(
            body,
            policy,
            crate::control::Tuning::default(),
            crate::control::SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..150 {
            let _ = runtime.tick();
        }
        assert_eq!(
            runtime.bringup(),
            crate::runtime::BringupState::Ready,
            "the duck must stand before the walk starts"
        );
        // 0.4 m/s straight ahead, the policy's forward gait. Long enough for
        // a 0.4 m/s gait to cross the 0.30 m to the scan's near face many
        // times over; the loop breaks early on the fall.
        runtime.set_twist([0.4, 0.0, 0.0]);
        let mut fallen = false;
        let mut walked = 0.0f64;
        for _ in 0..600 {
            let _ = runtime.tick();
            walked = runtime.io().trunk()[0];
            if runtime.fallen() {
                fallen = true;
                break;
            }
        }
        (fallen, walked)
    }

    /// The shim's contact buffer holds 512 pairs (`pairs[1024]` ints in
    /// `duck_mj_contact_geoms`). A crowded room can report more contacts than
    /// that in one step; reading `ncon` pairs would read past the buffer. The
    /// scan below deliberately overlaps free boxes so MuJoCo reports well
    /// over 512 contacts, and the count the core exposes must be the buffer's.
    #[test]
    fn a_crowded_room_never_reads_past_the_contact_buffer() {
        let scene = walking_scene();
        let mut scan = String::from("<mujoco model=\"contact-probe\"><worldbody>");
        let mut placed = 0;
        'grid: for x in 0..12 {
            for y in 0..12 {
                if placed >= 100 {
                    break 'grid;
                }
                let px = f64::from(x) * 0.03;
                let py = f64::from(y) * 0.03;
                scan.push_str(&format!(
                    "<body pos=\"{px:.3} {py:.3} 0.5\"><freejoint/>\
                     <geom type=\"box\" size=\"0.02 0.02 0.02\"/></body>"
                ));
                placed += 1;
            }
        }
        scan.push_str("</worldbody></mujoco>");

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("duck-crowded-scan-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("staging");
        std::fs::write(dir.join(OBSTACLES_FILE), scan).expect("write scan");

        let mut body = MujocoIo::load_with_obstacles(&scene, &dir).expect("the crowded room loads");
        body.step_once();
        let contacts = body.contacts();
        assert!(
            !contacts.is_empty(),
            "the overlapping boxes must produce contacts"
        );
        assert!(
            contacts.len() <= 512,
            "the shim buffer holds 512 pairs; contacts() returned {}",
            contacts.len()
        );
    }

    /// The scan is real physics, not a decoration. The official gait's own
    /// progress over a bounded run is not a reliable fall trigger, so this
    /// proves the geometry directly: the duck, held at its official home
    /// pose by the body's own hold, is swept forward on the ground plane in
    /// half-centimetre steps through the RobotIo seam. With the scan the
    /// swept duck *meets the furniture* — MuJoCo reports the obstacle-duck
    /// contacts; in the empty room the same sweep never sees one. MuJoCo is
    /// the only judge; no AR hardware anywhere.
    #[test]
    fn the_scanned_box_stops_the_duck_where_the_room_is_empty() {
        let scene = walking_scene();
        let staging = staged_obstacles();

        /// Sweep the held-pose duck forward; return how far it travelled
        /// before a geom of the scanned furniture touched it (None: never).
        /// `duck_mobile_core::mujoco::MujocoIo` pins the home pose whenever
        /// it is not released, so each swept step is the duck standing.
        let first_contact_x = |scan: Option<&std::path::Path>| -> Option<f64> {
            let mut body = match scan {
                Some(dir) => MujocoIo::load_with_obstacles(&scene, dir).expect("body with scan"),
                None => MujocoIo::load(&scene).expect("plain body"),
            };
            let mut touched = None;
            for step in 0..90u32 {
                let x = 0.005 * f64::from(step);
                // One 5 ms substep of the duck's own physics, from the swept
                // position. `MujocoIo::set_trunk_position` is the sweep hook
                // below: it writes the freejoint and re-runs forward.
                body.set_trunk_position([x, 0.0, HOME_TRUNK_Z]);
                body.step_once();
                if touched.is_none() && body.scanned_geom_touching() {
                    touched = Some(x);
                }
            }
            touched
        };

        assert_eq!(
            first_contact_x(None),
            None,
            "in the empty room the sweep meets nothing"
        );
        let contact = first_contact_x(Some(&staging))
            .expect("the scanned furniture must touch the swept duck");
        assert!(
            (0.25..=0.35).contains(&contact),
            "the scan's near face is 0.30 m ahead; first contact at {contact}"
        );
    }

    /// The roller scene must load through the same mapping the walking one
    /// uses: same trunk freejoint, the same 14 actuators by name, and passive
    /// wheel joints that are simply not in the wire table.
    #[test]
    fn the_roller_scene_loads_with_the_same_wire_mapping() {
        // The scene lives in the fetched upstream checkout; resolve it the way
        // every fixture test does, and let CI's DUCK_REQUIRE_FIXTURES=1 turn a
        // missing asset into a failure rather than a silent skip.
        let scene = crate::fixtures::workspace_root()
            .join("upstream/microduck_rl/src/mjlab_microduck/robot/microduck/scene_rollers.xml");
        if !scene.is_file() {
            if crate::fixtures::required() {
                panic!("the roller scene is missing at {}", scene.display());
            }
            eprintln!(
                "skipping: the roller scene is not here at {}",
                scene.display()
            );
            return;
        }
        let io = MujocoIo::load(&scene).expect("the roller scene loads");
        assert!(io.body_count() > 0);
        // Every leg, neck and head servo is present. The roller model has no
        // mouth joint, exactly like the walking one, so a beak intent is
        // refused there too rather than accepted and dropped.
        assert!(!io.has_mouth());
        assert_eq!(io.nu, 14, "the same fourteen servos as the walking model");
    }

    /// SENSORS S2: the AR floor is a real model edit. Moving the plane moves
    /// the geometry the renderer and the contacts see; the duck's senses are
    /// untouched.
    #[test]
    fn the_floor_hook_moves_the_plane_geom() {
        let scene = walking_scene();
        let mut io = MujocoIo::load(&scene).expect("the walking scene");
        let geom = io.set_floor_z(0.03).expect("the scene has a ground plane");
        let info = io.geom_info(geom).expect("the plane is a geom");
        assert_eq!(info.geom_type, 0, "mjtGeom plane");
        assert!((info.pos[2] - 0.03).abs() < 1e-9, "{:?}", info.pos);
        // Back to the training scene's floor; a second move is not a no-op.
        assert!(io.set_floor_z(0.0).is_some());
        assert!(io.geom_info(geom).unwrap().pos[2].abs() < 1e-9);
    }

    /// The route's dimension: an offset from the scene's own floor, with the
    /// duck moving too. A floor that rises under a duck that stays put is a
    /// duck pushed into the ground; this hook must not do that.
    #[test]
    fn the_floor_delta_moves_the_plane_and_the_duck_together() {
        let scene = walking_scene();
        let mut io = MujocoIo::load(&scene).expect("the walking scene");
        let base_trunk = io.trunk()[2];
        assert!((base_trunk - HOME_TRUNK_Z).abs() < 1e-9, "{base_trunk}");
        assert!((io.held_trunk[2] - HOME_TRUNK_Z).abs() < 1e-9);

        let geom = io.set_floor_delta(0.1).expect("a ground plane");
        assert!((io.geom_info(geom).unwrap().pos[2] - 0.1).abs() < 1e-9);
        assert!((io.trunk()[2] - (base_trunk + 0.1)).abs() < 1e-9);
        assert!((io.held_trunk[2] - (base_trunk + 0.1)).abs() < 1e-9);

        // Setting the offset from the base again, not from where it was: the
        // shells post a measurement, not an increment.
        let geom = io.set_floor_delta(-0.05).expect("a ground plane");
        assert!((io.geom_info(geom).unwrap().pos[2] + 0.05).abs() < 1e-9);
        assert!((io.trunk()[2] - (base_trunk - 0.05)).abs() < 1e-9);
        assert!((io.held_trunk[2] - (base_trunk - 0.05)).abs() < 1e-9);
    }

    /// A body with no plane has no floor to move: the hook answers `None`,
    /// and the route turns that into an honest refusal.
    #[test]
    fn a_scene_without_a_plane_has_no_floor_to_move() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("duck-no-plane-{}-{stamp}.xml", std::process::id()));
        // The smallest body the loader accepts: the trunk freejoint it maps
        // by name, and an inertial so the free body has a mass — MuJoCo
        // refuses a moving body without one, which is its own honesty, not
        // ours.
        std::fs::write(
            &path,
            "<mujoco><worldbody>\
             <body name=\"trunk_base\" pos=\"0 0 0.1\">\
             <freejoint name=\"trunk_base_freejoint\"/>\
             <inertial pos=\"0 0 0\" mass=\"0.5\" diaginertia=\"0.001 0.001 0.001\"/>\
             <geom size=\"0.05\"/>\
             </body></worldbody></mujoco>",
        )
        .expect("write the plane-less model");
        let mut io = MujocoIo::load(&path).expect("a body without a floor still loads");
        assert_eq!(io.set_floor_z(0.1), None);
        assert_eq!(io.set_floor_delta(0.1), None);
        let _ = std::fs::remove_file(&path);
    }
}

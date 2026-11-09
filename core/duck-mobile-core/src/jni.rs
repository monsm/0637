//! JNI for the Android shell.
//!
//! This file only marshals; every call lands on the same C ABI the iOS app
//! uses, so both platforms run the identical core.
#![allow(unsafe_op_in_unsafe_fn)]

#![allow(non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jdouble, jdoubleArray, jfloatArray, jint, jlong, jstring};
use jni::JNIEnv;

use crate::ffi::{
    duck_core_body_count, duck_core_next_sound, duck_core_render_sound, duck_core_voice_rate, duck_core_body_pose, duck_core_carry, duck_core_free,
    duck_core_geom_count, duck_core_geom_info, duck_core_ground_pick, duck_core_load_pickup,
    duck_core_mesh_name, duck_core_mesh_soup,
    duck_core_fork, duck_core_new, duck_core_paused, duck_core_seek, duck_core_set_paused,
    duck_core_set_speed, duck_core_speed, duck_core_step,
    duck_core_push, duck_core_reload_policies, duck_core_render_qpos, duck_core_set_body,
    duck_core_set_head, duck_core_set_twist, duck_core_sit_toggle, duck_core_skill_count,
    duck_core_skill_name, duck_core_start_intent_server, duck_core_status, duck_core_tick,
    duck_core_trigger_skill, DuckCore, DuckStatus,
};

fn java_string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    env.get_string(value).ok().map(|text| text.into())
}

fn double_array(env: &mut JNIEnv, values: &[f64]) -> jdoubleArray {
    let Ok(array) = env.new_double_array(values.len() as i32) else {
        return std::ptr::null_mut();
    };
    if env.set_double_array_region(&array, 0, values).is_err() {
        return std::ptr::null_mut();
    }
    array.into_raw()
}

fn empty_array(env: &mut JNIEnv) -> jdoubleArray {
    double_array(env, &[])
}

fn empty_floats(env: &mut JNIEnv) -> jfloatArray {
    match env.new_float_array(0) {
        Ok(array) => array.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe fn core(handle: jlong) -> Option<&'static mut DuckCore> {
    let pointer = handle as *mut DuckCore;
    pointer.as_mut()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeNew(
    mut env: JNIEnv,
    _class: JClass,
    scene: JString,
    policies: JString,
) -> jlong {
    let (Some(scene), Some(policies)) =
        (java_string(&mut env, &scene), java_string(&mut env, &policies))
    else {
        return 0;
    };
    let Ok(scene) = CString::new(scene) else {
        return 0;
    };
    let Ok(policies) = CString::new(policies) else {
        return 0;
    };
    let mut error = [0 as c_char; 1024];
    let core = unsafe {
        duck_core_new(
            scene.as_ptr(),
            policies.as_ptr(),
            error.as_mut_ptr(),
            error.len() as i32,
        )
    };
    core as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { duck_core_free(handle as *mut DuckCore) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeTick(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_tick(handle as *mut DuckCore) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSetTwist(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    vx: jdouble,
    vy: jdouble,
    wz: jdouble,
) {
    unsafe { duck_core_set_twist(handle as *mut DuckCore, vx, vy, wz) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSetHead(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    neck: jdouble,
    head: jdouble,
    yaw: jdouble,
    roll: jdouble,
) {
    unsafe { duck_core_set_head(handle as *mut DuckCore, neck, head, yaw, roll) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSetBody(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    z: jdouble,
    roll: jdouble,
    pitch: jdouble,
) {
    unsafe { duck_core_set_body(handle as *mut DuckCore, z, roll, pitch) };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeCarry(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    dx: jdouble,
    dy: jdouble,
    dz: jdouble,
    active: jboolean,
) {
    unsafe {
        duck_core_carry(
            handle as *mut DuckCore,
            dx,
            dy,
            dz,
            // `jboolean` is a u8, not Rust's bool.
            i32::from(active != 0),
        )
    };
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeLoadPickup(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    path: JString,
) -> jint {
    let Some(path) = java_string(&mut env, &path) else {
        return -1;
    };
    let Ok(path) = CString::new(path) else {
        return -1;
    };
    unsafe { duck_core_load_pickup(handle as *mut DuckCore, path.as_ptr()) as jint }
}

/// The next sound the duck wants, as a tag; `null` when none is queued.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeNextSound(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    let pointer = unsafe { duck_core_next_sound(handle as *mut DuckCore) };
    if pointer.is_null() {
        return std::ptr::null_mut();
    }
    let tag = unsafe { CStr::from_ptr(pointer) };
    match env.new_string(tag.to_string_lossy()) {
        Ok(value) => value.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// One sound as mono f32 PCM. Returns the samples, or an empty array.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeRenderSound(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    tag: JString,
) -> jfloatArray {
    let Some(tag) = java_string(&mut env, &tag) else {
        return empty_floats(&mut env);
    };
    let Ok(tag) = CString::new(tag) else {
        return empty_floats(&mut env);
    };
    let count = unsafe {
        duck_core_render_sound(handle as *mut DuckCore, tag.as_ptr(), std::ptr::null_mut(), 0)
    };
    if count <= 0 {
        return empty_floats(&mut env);
    }
    let mut samples = vec![0.0f32; count as usize];
    let written = unsafe {
        duck_core_render_sound(
            handle as *mut DuckCore,
            tag.as_ptr(),
            samples.as_mut_ptr(),
            count,
        )
    };
    if written <= 0 {
        return empty_floats(&mut env);
    }
    let Ok(array) = env.new_float_array(written) else {
        return empty_floats(&mut env);
    };
    if env
        .set_float_array_region(&array, 0, &samples[..written as usize])
        .is_err()
    {
        return empty_floats(&mut env);
    }
    array.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeVoiceRate(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    unsafe { duck_core_voice_rate() as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSkill(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    name: JString,
) -> jint {
    let Some(name) = java_string(&mut env, &name) else {
        return 0;
    };
    let Ok(name) = CString::new(name) else {
        return 0;
    };
    unsafe { duck_core_trigger_skill(handle as *mut DuckCore, name.as_ptr()) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSit(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_sit_toggle(handle as *mut DuckCore) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeGroundPick(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_ground_pick(handle as *mut DuckCore) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativePush(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    vx: jdouble,
    vy: jdouble,
    vz: jdouble,
) -> jint {
    unsafe { duck_core_push(handle as *mut DuckCore, vx, vy, vz) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeStartIntentServer(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    port: jint,
) -> jint {
    unsafe { duck_core_start_intent_server(handle as *mut DuckCore, port) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeReload(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    directory: JString,
) -> jint {
    let Some(directory) = java_string(&mut env, &directory) else {
        return -1;
    };
    let Ok(directory) = CString::new(directory) else {
        return -1;
    };
    unsafe { duck_core_reload_policies(handle as *mut DuckCore, directory.as_ptr()) as jint }
}

/// [bringup, fallen, gain, trunk x/y/z, gravity x/y/z]
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeStatus(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jdoubleArray {
    let mut status = DuckStatus {
        bringup: 0,
        fallen: 0,
        gain: 0,
        label: [0; 64],
        trunk: [0.0; 3],
        gravity: [0.0; 3],
        imu_ready: 0,
        picked_up: 0,
        p_held: 0.0,
        mouth: 0.0,
        volts: 0.0,
        has_mouth: 0,
    };
    if unsafe { duck_core_status(handle as *mut DuckCore, &mut status) } != 0 {
        return empty_array(&mut env);
    }
    let values = [
        status.bringup as f64,
        status.fallen as f64,
        status.gain as f64,
        status.trunk[0],
        status.trunk[1],
        status.trunk[2],
        status.gravity[0],
        status.gravity[1],
        status.gravity[2],
        status.imu_ready as f64,
        status.picked_up as f64,
        status.p_held,
        status.mouth,
        status.volts,
        status.has_mouth as f64,
    ];
    double_array(&mut env, &values)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeRenderQpos(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jdoubleArray {
    let Some(core) = (unsafe { core(handle) }) else {
        return empty_array(&mut env);
    };
    let count = unsafe { duck_core_render_qpos(core, std::ptr::null_mut(), 0) } as usize;
    let mut values = vec![0.0f64; count];
    unsafe { duck_core_render_qpos(core, values.as_mut_ptr(), count as i32) };
    double_array(&mut env, &values)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeBodyCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_body_count(handle as *mut DuckCore) as jint }
}

/// [x, y, z, qw, qx, qy, qz]
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeBodyPose(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    body: jint,
) -> jdoubleArray {
    let mut position = [0.0f64; 3];
    let mut rotation = [0.0f64; 4];
    if unsafe {
        duck_core_body_pose(
            handle as *mut DuckCore,
            body,
            position.as_mut_ptr(),
            rotation.as_mut_ptr(),
        )
    } != 0
    {
        return empty_array(&mut env);
    }
    let values = [
        position[0],
        position[1],
        position[2],
        rotation[0],
        rotation[1],
        rotation[2],
        rotation[3],
    ];
    double_array(&mut env, &values)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeGeomCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_geom_count(handle as *mut DuckCore) as jint }
}

/// [body, type, mesh, group, pos x/y/z, quat w/x/y/z, rgba r/g/b/a]
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeGeomInfo(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    geom: jint,
) -> jdoubleArray {
    let mut body = 0i32;
    let mut geom_type = 0i32;
    let mut mesh = -1i32;
    let mut group = 0i32;
    let mut position = [0.0f64; 3];
    let mut rotation = [0.0f64; 4];
    let mut rgba = [0.0f32; 4];
    if unsafe {
        duck_core_geom_info(
            handle as *mut DuckCore,
            geom,
            &mut body,
            &mut geom_type,
            &mut mesh,
            &mut group,
            position.as_mut_ptr(),
            rotation.as_mut_ptr(),
            rgba.as_mut_ptr(),
        )
    } != 0
    {
        return empty_array(&mut env);
    }
    let values = [
        body as f64,
        geom_type as f64,
        mesh as f64,
        group as f64,
        position[0],
        position[1],
        position[2],
        rotation[0],
        rotation[1],
        rotation[2],
        rotation[3],
        rgba[0] as f64,
        rgba[1] as f64,
        rgba[2] as f64,
        rgba[3] as f64,
    ];
    double_array(&mut env, &values)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSkillCount(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { duck_core_skill_count(handle as *mut DuckCore) as jint }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSkillName(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    index: jint,
) -> jni::sys::jstring {
    let pointer = unsafe { duck_core_skill_name(handle as *mut DuckCore, index) };
    if pointer.is_null() {
        return std::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned();
    env.new_string(name)
        .map(|string| string.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeMeshName(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    mesh: jint,
) -> jni::sys::jstring {
    let pointer = unsafe { duck_core_mesh_name(handle as *mut DuckCore, mesh) };
    if pointer.is_null() {
        return std::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned();
    env.new_string(name)
        .map(|string| string.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Triangle soup (px,py,pz,nx,ny,nz per vertex, three per face) in MuJoCo's
/// own mesh frame. Parsing the STL instead puts every part in the wrong
/// place: the compiler re-centres and re-orients each mesh.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeMeshSoup(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    mesh: jint,
) -> jni::sys::jfloatArray {
    let core = handle as *mut DuckCore;
    if core.is_null() || mesh < 0 {
        return std::ptr::null_mut();
    }
    let count = unsafe { duck_core_mesh_soup(core, mesh, std::ptr::null_mut(), 0) };
    if count <= 0 {
        return std::ptr::null_mut();
    }
    let mut soup = vec![0f32; count as usize];
    let written = unsafe { duck_core_mesh_soup(core, mesh, soup.as_mut_ptr(), count) };
    if written != count {
        return std::ptr::null_mut();
    }
    let Ok(array) = env.new_float_array(soup.len() as i32) else {
        return std::ptr::null_mut();
    };
    if env.set_float_array_region(&array, 0, &soup).is_err() {
        return std::ptr::null_mut();
    }
    array.into_raw()
}

/// Time machine: frozen?
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativePaused(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jni::sys::jboolean {
    (unsafe { duck_core_paused(handle as *mut DuckCore) } != 0) as jni::sys::jboolean
}

/// Time machine: ticks per wall-clock frame.
#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSpeed(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jdouble {
    unsafe { duck_core_speed(handle as *mut DuckCore) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSetPaused(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    paused: jni::sys::jboolean,
) {
    unsafe { duck_core_set_paused(handle as *mut DuckCore, if paused != 0 { 1 } else { 0 }) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSetSpeed(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    speed: jdouble,
) {
    unsafe { duck_core_set_speed(handle as *mut DuckCore, speed) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeStep(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    ticks: jint,
) {
    unsafe { duck_core_step(handle as *mut DuckCore, ticks) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeSeek(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    back_ticks: jint,
) -> jint {
    unsafe { duck_core_seek(handle as *mut DuckCore, back_ticks) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_ai_duck_microduck_DuckCore_nativeFork(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    label: JString,
) -> jint {
    let text = if label.is_null() {
        None
    } else {
        java_string(&mut env, &label)
    };
    match text {
        Some(value) => match CString::new(value) {
            Ok(c) => unsafe { duck_core_fork(handle as *mut DuckCore, c.as_ptr()) },
            Err(_) => -1,
        },
        None => unsafe { duck_core_fork(handle as *mut DuckCore, std::ptr::null()) },
    }
}

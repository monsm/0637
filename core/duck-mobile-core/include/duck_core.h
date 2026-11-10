#pragma once
/* C ABI between the Swift app and the Rust duck core. */

#ifdef __cplusplus
extern "C" {
#endif

typedef struct DuckCore DuckCore;

typedef struct DuckStatus {
    /* 0 limp, 1 homing, 2 ready. */
    int bringup;
    int fallen;
    int gain;
    char label[64];
    double trunk[3];
    double gravity[3];
    /* The loop's own state, as `robotd` publishes it. */
    int imu_ready;   /* 0 until the orientation filter has converged */
    int picked_up;   /* 1 while the pickup classifier says a hand holds it */
    double p_held;   /* the classifier's latest score, 0 when not running */
    double mouth;    /* beak opening commanded, 0..1 */
    double volts;    /* pack voltage, 0 when not read yet */
    int has_mouth;   /* 1 when the scene has a mouth joint to move */
    /* The platform's own measurements, once a second: display frames per
       second and control ticks per second achieved (target: 50). */
    double fps;
    double ticks_per_second;
} DuckStatus;

DuckCore* duck_core_new(const char* scene_path, const char* policies_dir,
                        char* error, int error_size);
void duck_core_free(DuckCore* core);
int duck_core_reload_policies(DuckCore* core, const char* policies_dir);
int duck_core_start_intent_server(DuckCore* core, int port);

int duck_core_enable(DuckCore* core);
int duck_core_disable(DuckCore* core);
int duck_core_tick(DuckCore* core);

void duck_core_set_twist(DuckCore* core, double vx, double vy, double wz);
void duck_core_set_head(DuckCore* core, double neck, double head,
                        double yaw, double roll);
void duck_core_set_body(DuckCore* core, double z, double roll, double pitch);
/* Let go of the body pose (the pad's pose-mode exit). While a pose is held the
   controller zeroes the twist. */
void duck_core_release_body(DuckCore* core);
/* A hand on the trunk: metres from where it was grabbed; active 0 lets go. */
void duck_core_carry(DuckCore* core, double dx, double dy, double dz, int active);
int duck_core_push(DuckCore* core, double vx, double vy, double vz);

/* Bumped when the body model changed (a mode switch). Renderers that cache
   geometry rebuild when it moves. */
unsigned long long duck_core_model_generation(DuckCore* core);

/* The platform's own measurements, once a second: frames drawn per second and
   control ticks achieved per second. Diagnostic, not part of the duck. */
void duck_core_set_performance(DuckCore* core, double fps, double tps);

/* robot.flashlight on a phone; returns 0 off, 1..7 the colour index + 1. */
int duck_core_flashlight(DuckCore* core, int toggle, int on, const char* color);

/* Load robotd's emotion timelines from a bundle directory; returns the count. */
int duck_core_load_emotions(DuckCore* core, const char* dir);

/* The drive mode currently loaded: "walk" or "roller". */
const char* duck_core_mode(DuckCore* core);

/* Switch drive bundles: "walk" or "roller". 0 on success. */
int duck_core_set_mode(DuckCore* core, const char* mode);

/* The pickup classifier (`pickup_detector.onnx`). 0 on success. */
int duck_core_load_pickup(DuckCore* core, const char* model_path);

int duck_core_trigger_skill(DuckCore* core, const char* name);
int duck_core_sit_toggle(DuckCore* core);
int duck_core_ground_pick(DuckCore* core);

int duck_core_skill_count(DuckCore* core);
const char* duck_core_skill_name(DuckCore* core, int index);

int duck_core_status(DuckCore* core, DuckStatus* out);

int duck_core_render_qpos(DuckCore* core, double* out, int capacity);

int duck_core_body_count(DuckCore* core);
int duck_core_body_pose(DuckCore* core, int body, double* pos, double* quat);

int duck_core_geom_count(DuckCore* core);
int duck_core_geom_info(DuckCore* core, int geom, int* body, int* geom_type,
                        int* mesh, int* group, double* pos, double* quat,
                        float* rgba);

const char* duck_core_mesh_name(DuckCore* core, int mesh);

/* Time machine controls: freeze, speed, single-step, rewind, fork. */
void duck_core_set_paused(DuckCore* core, int paused);
void duck_core_set_speed(DuckCore* core, double speed);
void duck_core_step(DuckCore* core, int ticks);
int duck_core_seek(DuckCore* core, int back_ticks);
int duck_core_fork(DuckCore* core, const char* label);

/* Time machine: frozen?, ticks per frame, and the current tick index. */
int duck_core_paused(DuckCore* core);
double duck_core_speed(DuckCore* core);
unsigned long long duck_core_tick_index(DuckCore* core);
unsigned long long duck_core_oldest_tick(DuckCore* core);

/* The voice: a tag to play (NULL when nothing is queued), the PCM for a tag
   at `duck_core_voice_rate()` (48000), and the rate itself. The duck's voice
   is the robot's own synth, so a platform renders rather than ships samples. */
const char* duck_core_next_sound(DuckCore* core);
int duck_core_queue_sound(DuckCore* core, const char* tag);
int duck_core_render_sound(DuckCore* core, const char* tag, float* out, int capacity);
int duck_core_voice_rate(void);

/* One hash over the scene, meshes, policies and manifest. */
const char* duck_core_fingerprint(DuckCore* core);

/* Triangle soup (px,py,pz,nx,ny,nz per vertex) in MuJoCo's mesh frame.
   Returns the float count; fills `out` when it is non-null and large
   enough. Use this instead of parsing the STL files. */
int duck_core_mesh_soup(DuckCore* core, int mesh, float* out, int capacity);

#ifdef __cplusplus
}
#endif

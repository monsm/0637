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
int duck_core_push(DuckCore* core, double vx, double vy, double vz);

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

/* Triangle soup (px,py,pz,nx,ny,nz per vertex) in MuJoCo's mesh frame.
   Returns the float count; fills `out` when it is non-null and large
   enough. Use this instead of parsing the STL files. */
int duck_core_mesh_soup(DuckCore* core, int mesh, float* out, int capacity);

#ifdef __cplusplus
}
#endif

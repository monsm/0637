#include "duck_mj_shim.h"

#include <mujoco/mujoco.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(__ANDROID__)
/* bionic's libdl.a is the *static executable* shim. Its __cxa_atexit reads
   __libc_shared_globals()->atexit, which is never initialised in a shared
   library, so every C++ static constructor crashed at dlopen. Defining the
   symbol here keeps that archive member from being pulled at all; the
   registered destructors run at process exit. */
typedef void (*duck_atexit_fn)(void*);
#define DUCK_ATEXIT_MAX 256
static duck_atexit_fn g_atexit_fns[DUCK_ATEXIT_MAX];
static void* g_atexit_args[DUCK_ATEXIT_MAX];
static int g_atexit_count = 0;

static void duck_run_atexit(void) {
    while (g_atexit_count > 0) {
        int i = --g_atexit_count;
        g_atexit_fns[i](g_atexit_args[i]);
    }
}

int __cxa_atexit(duck_atexit_fn fn, void* arg, void* dso) {
    (void)dso;
    if (g_atexit_count >= DUCK_ATEXIT_MAX) {
        return -1;
    }
    g_atexit_fns[g_atexit_count] = fn;
    g_atexit_args[g_atexit_count] = arg;
    if (g_atexit_count++ == 0) {
        atexit(duck_run_atexit);
    }
    return 0;
}
#endif

struct DuckMj {
    mjModel* m;
    mjData* d;
};

DuckMj* duck_mj_load(const char* xml_path, char* error, int error_size) {
    char err[1000] = {0};
    mjModel* m = mj_loadXML(xml_path, NULL, err, sizeof(err));
    if (!m) {
        if (error && error_size > 0) snprintf(error, error_size, "%s", err[0] ? err : "mj_loadXML failed");
        return NULL;
    }
    mjData* d = mj_makeData(m);
    if (!d) {
        mj_deleteModel(m);
        if (error && error_size > 0) snprintf(error, error_size, "mj_makeData failed");
        return NULL;
    }
    DuckMj* h = (DuckMj*)calloc(1, sizeof(DuckMj));
    if (!h) {
        mj_deleteData(d);
        mj_deleteModel(m);
        if (error && error_size > 0) snprintf(error, error_size, "out of memory");
        return NULL;
    }
    h->m = m;
    h->d = d;
    return h;
}

void duck_mj_free(DuckMj* h) {
    if (!h) return;
    mj_deleteData(h->d);
    mj_deleteModel(h->m);
    free(h);
}

int duck_mj_nq(const DuckMj* h) { return h->m->nq; }
int duck_mj_nv(const DuckMj* h) { return h->m->nv; }
int duck_mj_nu(const DuckMj* h) { return h->m->nu; }
int duck_mj_nbody(const DuckMj* h) { return h->m->nbody; }
int duck_mj_ngeom(const DuckMj* h) { return h->m->ngeom; }
int duck_mj_nmesh(const DuckMj* h) { return h->m->nmesh; }
int duck_mj_nkey(const DuckMj* h) { return h->m->nkey; }
double duck_mj_timestep(const DuckMj* h) { return h->m->opt.timestep; }
void duck_mj_set_timestep(DuckMj* h, double dt) { h->m->opt.timestep = dt; }

double* duck_mj_qpos(DuckMj* h) { return h->d->qpos; }
double* duck_mj_qvel(DuckMj* h) { return h->d->qvel; }
double* duck_mj_ctrl(DuckMj* h) { return h->d->ctrl; }
double* duck_mj_actuator_force(DuckMj* h) { return h->d->actuator_force; }
double* duck_mj_xpos(DuckMj* h) { return h->d->xpos; }
double* duck_mj_xquat(DuckMj* h) { return h->d->xquat; }
float* duck_mj_geom_rgba(DuckMj* h) { return h->m->geom_rgba; }
double* duck_mj_geom_pos(DuckMj* h) { return h->m->geom_pos; }
double* duck_mj_geom_quat(DuckMj* h) { return h->m->geom_quat; }
double* duck_mj_geom_size(DuckMj* h) { return h->m->geom_size; }
int duck_mj_geom_type(const DuckMj* h, int geom) { return h->m->geom_type[geom]; }
int duck_mj_geom_bodyid(const DuckMj* h, int geom) { return h->m->geom_bodyid[geom]; }
int duck_mj_geom_meshid(const DuckMj* h, int geom) { return h->m->geom_dataid[geom]; }
int duck_mj_geom_group(const DuckMj* h, int geom) { return h->m->geom_group[geom]; }

int duck_mj_joint_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_JOINT, name); }
int duck_mj_body_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_BODY, name); }
int duck_mj_geom_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_GEOM, name); }
int duck_mj_actuator_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_ACTUATOR, name); }
int duck_mj_mesh_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_MESH, name); }
int duck_mj_site_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_SITE, name); }
int duck_mj_key_id(const DuckMj* h, const char* name) { return mj_name2id(h->m, mjOBJ_KEY, name); }

const char* duck_mj_body_name(const DuckMj* h, int body) { return mj_id2name(h->m, mjOBJ_BODY, body); }
const char* duck_mj_mesh_name(const DuckMj* h, int mesh) { return mj_id2name(h->m, mjOBJ_MESH, mesh); }
const char* duck_mj_joint_name(const DuckMj* h, int joint) { return mj_id2name(h->m, mjOBJ_JOINT, joint); }

int duck_mj_jnt_qposadr(const DuckMj* h, int joint) { return h->m->jnt_qposadr[joint]; }
int duck_mj_jnt_dofadr(const DuckMj* h, int joint) { return h->m->jnt_dofadr[joint]; }
int duck_mj_actuator_trnid(const DuckMj* h, int actuator, int index) {
    return h->m->actuator_trnid[actuator * 2 + index];
}

void duck_mj_set_position_gain(DuckMj* h, int actuator, double kp) {
    if (actuator < 0 || actuator >= h->m->nu) return;
    h->m->actuator_gainprm[actuator * mjNGAIN + 0] = kp;
    h->m->actuator_biasprm[actuator * mjNBIAS + 1] = -kp;
}

double duck_mj_position_gain(const DuckMj* h, int actuator) {
    if (actuator < 0 || actuator >= h->m->nu) return 0.0;
    return h->m->actuator_gainprm[actuator * mjNGAIN + 0];
}

void duck_mj_make_motor(DuckMj* h, int actuator) {
    if (actuator < 0 || actuator >= h->m->nu) return;
    h->m->actuator_gaintype[actuator] = mjGAIN_FIXED;
    h->m->actuator_biastype[actuator] = mjBIAS_NONE;
    h->m->actuator_dyntype[actuator] = mjDYN_NONE;
    for (int i = 0; i < mjNGAIN; i++) h->m->actuator_gainprm[actuator * mjNGAIN + i] = 0.0;
    for (int i = 0; i < mjNBIAS; i++) h->m->actuator_biasprm[actuator * mjNBIAS + i] = 0.0;
    for (int i = 0; i < 6; i++) h->m->actuator_gear[actuator * 6 + i] = 0.0;
    h->m->actuator_gainprm[actuator * mjNGAIN + 0] = 1.0;
    h->m->actuator_gear[actuator * 6 + 0] = 1.0;
    h->m->actuator_ctrllimited[actuator] = 0;
    h->m->actuator_forcelimited[actuator] = 1;
}

void duck_mj_set_force_limit(DuckMj* h, int actuator, double limit) {
    if (actuator < 0 || actuator >= h->m->nu) return;
    h->m->actuator_forcerange[actuator * 2 + 0] = -limit;
    h->m->actuator_forcerange[actuator * 2 + 1] = limit;
}

void duck_mj_zero_joint_friction(DuckMj* h, int joint, double armature,
                                 double solref0, double solref1,
                                 double solimp0, double solimp1, double solimp2,
                                 double solimp3, double solimp4) {
    if (joint < 0 || joint >= h->m->njnt) return;
    int dof = h->m->jnt_dofadr[joint];
    h->m->dof_armature[dof] = armature;
    h->m->dof_damping[dof] = 0.0;
    h->m->dof_frictionloss[dof] = 0.0;
    h->m->jnt_solref[joint * 2 + 0] = solref0;
    h->m->jnt_solref[joint * 2 + 1] = solref1;
    h->m->jnt_solimp[joint * 5 + 0] = solimp0;
    h->m->jnt_solimp[joint * 5 + 1] = solimp1;
    h->m->jnt_solimp[joint * 5 + 2] = solimp2;
    h->m->jnt_solimp[joint * 5 + 3] = solimp3;
    h->m->jnt_solimp[joint * 5 + 4] = solimp4;
}

void duck_mj_set_dof_friction(DuckMj* h, int dof, double frictionloss, double damping) {
    if (dof < 0 || dof >= h->m->nv) return;
    h->m->dof_frictionloss[dof] = frictionloss;
    h->m->dof_damping[dof] = damping;
}

double duck_mj_external_torque(DuckMj* h, int dof) {
    if (dof < 0 || dof >= h->m->nv) return 0.0;
    double friction = 0.0;
    for (int i = 0; i < h->d->nefc; i++) {
        if (h->d->efc_id[i] == dof && h->d->efc_type[i] == mjCNSTR_FRICTION_DOF) {
            friction += h->d->efc_force[i];
        }
    }
    return -h->d->qfrc_bias[dof] + h->d->qfrc_constraint[dof] - friction;
}

double duck_mj_actuator_torque(DuckMj* h, int dof) {
    if (dof < 0 || dof >= h->m->nv) return 0.0;
    return h->d->qfrc_actuator[dof];
}

void duck_mj_set_ctrl(DuckMj* h, int actuator, double value) {
    if (actuator < 0 || actuator >= h->m->nu) return;
    h->d->ctrl[actuator] = value;
}

void duck_mj_set_trunk_velocity(DuckMj* h, double vx, double vy, double vz) {
    int joint = mj_name2id(h->m, mjOBJ_JOINT, "trunk_base_freejoint");
    if (joint < 0) return;
    int dof = h->m->jnt_dofadr[joint];
    h->d->qvel[dof + 0] = vx;
    h->d->qvel[dof + 1] = vy;
    h->d->qvel[dof + 2] = vz;
}

void duck_mj_set_const(DuckMj* h) { mj_setConst(h->m, h->d); }

void duck_mj_reset(DuckMj* h) { mj_resetData(h->m, h->d); }
void duck_mj_reset_keyframe(DuckMj* h, int key) { mj_resetDataKeyframe(h->m, h->d, key); }
void duck_mj_forward(DuckMj* h) { mj_forward(h->m, h->d); }
void duck_mj_step(DuckMj* h) { mj_step(h->m, h->d); }
void duck_mj_quat2mat(double* mat, const double* quat) { mju_quat2Mat(mat, quat); }

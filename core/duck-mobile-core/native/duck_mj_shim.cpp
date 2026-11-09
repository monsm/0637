#include "duck_mj_shim.h"

#include <cstddef> // std::byte, used by mjspec.h in C++ builds
#include <mujoco/mujoco.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdexcept>

struct DuckMj {
    mjModel* m;
    mjData* d;
};

// Internal MuJoCo entry point (engine_io.c); not in the public headers but
// exported by the archive, used here only to split the makeData stages.
extern "C" void mj_makeRawData(mjData** dest, const mjModel* m);

DuckMj* duck_mj_load_impl(const char* xml_path, char* error, int error_size);

DuckMj* duck_mj_load(const char* xml_path, char* error, int error_size) {
    /* MuJoCo throws C++ exceptions (mjCError and friends); a C++ exception
       crossing into Rust aborts the process as a foreign exception. Guard
       every MuJoCo call here and surface the message through the error
       buffer instead. */
    try {
        return duck_mj_load_impl(xml_path, error, error_size);
    } catch (const std::exception& e) {
        snprintf(error, error_size, "mujoco exception: %s", e.what());
        return nullptr;
    } catch (...) {
        snprintf(error, error_size, "unknown mujoco exception");
        return nullptr;
    }
}

#if defined(__ANDROID__)
#include <android/log.h>
#define DUCK_LOG(...) __android_log_print(ANDROID_LOG_INFO, "duck-rust", __VA_ARGS__)
#else
#define DUCK_LOG(...) ((void)0)
#endif
#define DUCK_STAGE(name) DUCK_LOG("mujoco %s", name)

DuckMj* duck_mj_load_impl(const char* xml_path, char* error, int error_size) {
    char err[1000] = {0};
    DUCK_STAGE("loadXML begin");
    mjModel* m = mj_loadXML(xml_path, NULL, err, sizeof(err));
    DUCK_STAGE("loadXML end");
    if (!m) {
        if (error && error_size > 0) snprintf(error, error_size, "%s", err[0] ? err : "mj_loadXML failed");
        DUCK_LOG("loadXML failed: %s", err[0] ? err : "(no message)");
        return NULL;
    }
    DUCK_STAGE("makeData begin");
    /* mj_makeData sizes every arena from the model; log what it is about to
       allocate before calling it, so a hang or OOM is attributable. */
    DUCK_STAGE("makeData sizes");
    DUCK_LOG(
                        "mujoco model: nq=%d nv=%d nu=%d nbody=%d ngeom=%d "
                        "nmesh=%d nmeshvert=%d nmeshface=%d nmeshgraph=%d "
                        "nflexvert=%d neq=%d nkey=%d",
                        m->nq, m->nv, m->nu, m->nbody, m->ngeom, m->nmesh,
                        m->nmeshvert, m->nmeshface, m->nmeshgraph,
                        m->nflexvert, m->neq, m->nkey);
    DUCK_STAGE("makeData call");
    /* Bisect the hang: allocate a throwaway data for a minimal model first.
       If this returns, the deadlock is size/arena specific. */
    {
        char tiny_xml[] = "<mujoco><worldbody><body><freejoint/>"
                          "<geom size=\"0.1\"/></body></worldbody></mujoco>";
        char tiny_err[512] = {0};
        mjModel* tm = mj_loadXML(tiny_xml, NULL, tiny_err, sizeof(tiny_err));
        if (tm) {
            DUCK_STAGE("tiny makeData call");
            mjData* td = mj_makeData(tm);
            DUCK_STAGE("tiny makeData end");
            if (td) mj_deleteData(td);
            mj_deleteModel(tm);
        } else {
            DUCK_LOG("tiny model failed: %s", tiny_err);
        }
    }
    mjData* d = NULL;
    DUCK_STAGE("makeRawData call");
    mj_makeRawData(&d, m);
    DUCK_STAGE("makeRawData end");
    if (d) {
        DUCK_STAGE("resetData call");
        mj_resetData(m, d);
        DUCK_STAGE("resetData end");
    }
    DUCK_STAGE("makeData end");
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
// The colour MuJoCo's own renderer uses: the material's rgba when the geom
// has one, the geom's own rgba otherwise. geom_rgba alone is the default
// grey for every geom that references a material.
void duck_mj_geom_colour(const DuckMj* h, int geom, float* out) {
  const int mat = h->m->geom_matid[geom];
  const float* src = (mat >= 0) ? &h->m->mat_rgba[4 * mat] : &h->m->geom_rgba[4 * geom];
  out[0] = src[0];
  out[1] = src[1];
  out[2] = src[2];
  out[3] = src[3];
}
double* duck_mj_geom_pos(DuckMj* h) { return h->m->geom_pos; }
double* duck_mj_geom_quat(DuckMj* h) { return h->m->geom_quat; }
double* duck_mj_geom_size(DuckMj* h) { return h->m->geom_size; }
int duck_mj_geom_type(const DuckMj* h, int geom) { return h->m->geom_type[geom]; }
int duck_mj_geom_bodyid(const DuckMj* h, int geom) { return h->m->geom_bodyid[geom]; }
int duck_mj_geom_meshid(const DuckMj* h, int geom) { return h->m->geom_dataid[geom]; }
int duck_mj_geom_group(const DuckMj* h, int geom) { return h->m->geom_group[geom]; }
const char* duck_mj_version(void) { return mj_versionString(); }
void duck_mj_set_solver(DuckMj* h, int iterations, int ls_iterations) {
  if (!h) return;
  if (iterations > 0) h->m->opt.iterations = iterations;
  if (ls_iterations > 0) h->m->opt.ls_iterations = ls_iterations;
}
const float* duck_mj_mesh_vert(const DuckMj* h) { return h->m->mesh_vert; }
const int* duck_mj_mesh_vertadr(const DuckMj* h) { return h->m->mesh_vertadr; }
const int* duck_mj_mesh_vertnum(const DuckMj* h) { return h->m->mesh_vertnum; }
const int* duck_mj_mesh_face(const DuckMj* h) { return h->m->mesh_face; }
const int* duck_mj_mesh_faceadr(const DuckMj* h) { return h->m->mesh_faceadr; }
const int* duck_mj_mesh_facenum(const DuckMj* h) { return h->m->mesh_facenum; }

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

#pragma once
/* Stable C shim over MuJoCo 3.x. The Rust core calls only these symbols, so
 * the MuJoCo version and struct layout stay on this side of the boundary.
 * Built as a static library for iOS and Windows. */

#ifdef __cplusplus
extern "C" {
#endif

typedef struct DuckMj DuckMj;

DuckMj* duck_mj_load(const char* xml_path, char* error, int error_size);
void duck_mj_free(DuckMj* h);

int duck_mj_nq(const DuckMj* h);
int duck_mj_nv(const DuckMj* h);
int duck_mj_nu(const DuckMj* h);
int duck_mj_nbody(const DuckMj* h);
int duck_mj_ngeom(const DuckMj* h);
int duck_mj_nmesh(const DuckMj* h);
int duck_mj_nkey(const DuckMj* h);
double duck_mj_timestep(const DuckMj* h);
void duck_mj_set_timestep(DuckMj* h, double dt);

double* duck_mj_qpos(DuckMj* h);
double* duck_mj_qvel(DuckMj* h);
double* duck_mj_ctrl(DuckMj* h);
double* duck_mj_actuator_force(DuckMj* h);
double* duck_mj_xpos(DuckMj* h);
double* duck_mj_xquat(DuckMj* h);
float* duck_mj_geom_rgba(DuckMj* h);
double* duck_mj_geom_pos(DuckMj* h);
double* duck_mj_geom_quat(DuckMj* h);
double* duck_mj_geom_size(DuckMj* h);
int duck_mj_geom_type(const DuckMj* h, int geom);
int duck_mj_geom_bodyid(const DuckMj* h, int geom);
int duck_mj_geom_meshid(const DuckMj* h, int geom);
int duck_mj_geom_group(const DuckMj* h, int geom);
/* Resolved colour: the material's rgba when the geom has one. */
void duck_mj_geom_colour(const DuckMj* h, int geom, float* out);
/* MuJoCo's own mesh data, after the compiler's mesh frame. */
const float* duck_mj_mesh_vert(const DuckMj* h);
const int* duck_mj_mesh_vertadr(const DuckMj* h);
const int* duck_mj_mesh_vertnum(const DuckMj* h);
const int* duck_mj_mesh_face(const DuckMj* h);
const int* duck_mj_mesh_faceadr(const DuckMj* h);
const int* duck_mj_mesh_facenum(const DuckMj* h);

int duck_mj_joint_id(const DuckMj* h, const char* name);
int duck_mj_body_id(const DuckMj* h, const char* name);
int duck_mj_geom_id(const DuckMj* h, const char* name);
int duck_mj_actuator_id(const DuckMj* h, const char* name);
int duck_mj_mesh_id(const DuckMj* h, const char* name);
int duck_mj_site_id(const DuckMj* h, const char* name);
int duck_mj_key_id(const DuckMj* h, const char* name);

const char* duck_mj_body_name(const DuckMj* h, int body);
const char* duck_mj_mesh_name(const DuckMj* h, int mesh);
const char* duck_mj_joint_name(const DuckMj* h, int joint);

int duck_mj_jnt_qposadr(const DuckMj* h, int joint);
int duck_mj_jnt_dofadr(const DuckMj* h, int joint);
int duck_mj_actuator_trnid(const DuckMj* h, int actuator, int index);

void duck_mj_set_position_gain(DuckMj* h, int actuator, double kp);
double duck_mj_position_gain(const DuckMj* h, int actuator);

/* BAM support: turn a position actuator into a torque motor, zero the joint's
 * built-in friction, and expose the torque terms the friction model needs. */
void duck_mj_make_motor(DuckMj* h, int actuator);
void duck_mj_set_force_limit(DuckMj* h, int actuator, double limit);
void duck_mj_zero_joint_friction(DuckMj* h, int joint, double armature,
                                 double solref0, double solref1,
                                 double solimp0, double solimp1, double solimp2,
                                 double solimp3, double solimp4);
void duck_mj_set_dof_friction(DuckMj* h, int dof, double frictionloss, double damping);
double duck_mj_external_torque(DuckMj* h, int dof);
double duck_mj_actuator_torque(DuckMj* h, int dof);
void duck_mj_set_ctrl(DuckMj* h, int actuator, double value);
void duck_mj_set_trunk_velocity(DuckMj* h, double vx, double vy, double vz);
void duck_mj_set_const(DuckMj* h);
void duck_mj_reset(DuckMj* h);
void duck_mj_reset_keyframe(DuckMj* h, int key);
void duck_mj_forward(DuckMj* h);
void duck_mj_step(DuckMj* h);
void duck_mj_quat2mat(double* mat, const double* quat);

#ifdef __cplusplus
}
#endif

/* libratchet C API. Matches crates/libratchet-ffi/src/lib.rs; keep the two in sync.
 *
 * Coordinates: Y up, metres. Skyrim hosts convert (Z up, ~70 units per metre).
 * Each tick: fill RatchetInput + RatchetContact, call ratchet_step, move the character by
 * result.velocity * dt with your own collision, then report the new contacts next tick.
 */
#ifndef RATCHET_H
#define RATCHET_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct RatchetSim RatchetSim;

typedef struct RatchetVec3 {
    float x, y, z;
} RatchetVec3;

typedef struct RatchetInput {
    float move_x, move_z; /* world-space direction on the ground plane, length 0..1 */
    uint8_t jump;         /* jump button held */
    uint8_t dash;         /* dash button held */
    uint8_t melee;        /* melee (wrench) button held */
    uint8_t aiming;       /* face (aim_x, aim_z) and strafe: 1 = aim mode (aim button held: slower,
                             aim-mode jumps), 2 = strafing while firing from the hip (normal speeds) */
    float aim_x, aim_z;
    float view_right_x, view_right_z; /* the camera's right on the ground plane (zero without one):
                                         wall jumps take their side from it */
} RatchetInput;

typedef struct RatchetContact {
    uint8_t grounded;
    uint8_t hit_ceiling;
    uint8_t has_ground_distance;
    float ground_distance; /* feet to ground, metres */
    uint8_t has_wall;      /* the last move ran into a wall */
    uint8_t wall_sticky;   /* that wall takes wall jumps */
    float wall_normal_x;   /* out of the wall, on the ground plane */
    float wall_normal_z;
} RatchetContact;

enum {
    RATCHET_STATE_GROUND = 0,
    RATCHET_STATE_JUMP = 1,
    RATCHET_STATE_DOUBLE_JUMP = 2,
    RATCHET_STATE_FALL = 3,
    RATCHET_STATE_GLIDE = 4,
    RATCHET_STATE_DASH = 5,
    RATCHET_STATE_AIR_DASH = 6,
    RATCHET_STATE_MELEE = 7,      /* a combo swing; see melee_swing */
    RATCHET_STATE_SMASH = 8,      /* Hyper-Strike: hop, then slam down */
    RATCHET_STATE_SMASH_LAND = 9, /* on the ground after the slam */
    RATCHET_STATE_WALL_STICK = 10, /* stuck to a wall-jump wall, then sliding down */
    RATCHET_STATE_WALL_JUMP = 11,  /* the jump off it */
};

enum {
    RATCHET_EVENT_JUMPED = 1u << 0,
    RATCHET_EVENT_DOUBLE_JUMPED = 1u << 1,
    RATCHET_EVENT_LEFT_GROUND = 1u << 2,
    RATCHET_EVENT_GLIDE_STARTED = 1u << 3,
    RATCHET_EVENT_GLIDE_ENDED = 1u << 4,
    RATCHET_EVENT_LANDED = 1u << 5,
    RATCHET_EVENT_DASHED = 1u << 6,
    RATCHET_EVENT_AIR_DASHED = 1u << 7,
    RATCHET_EVENT_MELEE_SWING = 1u << 8,   /* a swing started (melee_swing) */
    RATCHET_EVENT_MELEE_HIT = 1u << 9,     /* the swing connects now: check for hits */
    RATCHET_EVENT_SMASH_STARTED = 1u << 10,
    RATCHET_EVENT_SMASH_IMPACT = 1u << 11, /* the slam hit the ground: see ratchet_smash_info */
    RATCHET_EVENT_WALL_STUCK = 1u << 12,   /* stuck to a wall that takes wall jumps */
    RATCHET_EVENT_WALL_JUMPED = 1u << 13,  /* jumped off it */
};

typedef struct RatchetStepResult {
    RatchetVec3 velocity;    /* move by velocity * dt */
    uint32_t state;          /* RATCHET_STATE_* */
    uint32_t events;         /* RATCHET_EVENT_* bits */
    float facing_yaw;        /* radians, atan2(x, z) */
    float landed_fall_speed; /* valid when RATCHET_EVENT_LANDED is set */
    uint32_t melee_swing;    /* combo swing (0 = first) in progress or of this tick's swing/hit event */
    uint8_t wall_right;      /* on a wall or in a wall jump: 1 if the game's right wall clips fit, 0 left */
} RatchetStepResult;

typedef struct RatchetSwingInfo {
    float duration;
    float hit_time;  /* seconds into the swing when it connects */
    float damage;
    float knockback; /* how far a standard enemy is knocked back, metres */
    float early_exit; /* from here the next swing may start and the stick may cut this one short */
    float exit;       /* the swing is over here */
} RatchetSwingInfo;

typedef struct RatchetWrench RatchetWrench;

/* Generic default tuning, scaled (1 = Ratchet-sized). Free with ratchet_free. */
RatchetSim *ratchet_new(float scale);
void ratchet_free(RatchetSim *sim);

/* Load extracted Rift Apart config JSON from dir (UTF-8). 0 = ok; on error the old tuning stays. */
int32_t ratchet_load_configs(RatchetSim *sim, const char *dir, float scale);
void ratchet_set_scale(RatchetSim *sim, float scale);

RatchetStepResult ratchet_step(RatchetSim *sim, float dt, RatchetInput input, RatchetContact contact, RatchetVec3 feet);
void ratchet_set_velocity(RatchetSim *sim, RatchetVec3 velocity);
void ratchet_reset(RatchetSim *sim);

/* Combo swing `index` into out; returns the number of swings, or 0 if index is out of range. */
uint32_t ratchet_swing_info(const RatchetSim *sim, uint32_t index, RatchetSwingInfo *out);
/* Hyper-Strike slam damage and radius (metres). Either pointer may be null. */
void ratchet_smash_info(const RatchetSim *sim, float *damage, float *radius);

/* Thrown OmniWrench: out along dir, back to the hand. Step it each tick with the hand position
 * and blocked = 1 if it hit something; it returns 1 once caught (then free it). */
RatchetWrench *ratchet_wrench_throw(const RatchetSim *sim, RatchetVec3 from, RatchetVec3 dir);
uint8_t ratchet_wrench_step(RatchetWrench *wrench, float dt, RatchetVec3 hand, uint8_t blocked, RatchetVec3 *position, float *spin);
void ratchet_wrench_free(RatchetWrench *wrench);

#ifdef __cplusplus
}
#endif

#endif

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
                                         wall jumps take their side from it; the Swingshot frames
                                         its view and pumps by it (from view_forward when zero) */
    uint8_t crouch;       /* crouch (the ledge drop button) held */
    uint8_t swingshot;    /* the Swingshot button held: pressed with a target in view, he swings on it
                             (or the Hookshot reels him in) while it's held */
    float view_forward_x, view_forward_y, view_forward_z; /* where the camera looks (unit; zero without
                                                             one): Swingshot targets are picked in its view */
} RatchetInput;

typedef struct RatchetContact {
    uint8_t grounded;
    uint8_t hit_ceiling;
    uint8_t has_ground_distance;
    float ground_distance; /* feet to ground, metres */
    uint8_t has_wall;      /* a wall beside him: the nearest within about 1 m (x scale, with
                              wall_distance), or one the last move ran into; on a wall (state 10),
                              that one (facing result.wall_normal) when it's in reach */
    uint8_t wall_sticky;   /* a wall-jump wall (marked in the level): any number of wall jumps;
                              plain walls take one per jump */
    float wall_normal_x;   /* out of the wall, on the ground plane */
    float wall_normal_z;
    uint8_t wall_jump_zone; /* inside a level's wall-jump zone: wall jumps keep their line there */
    uint8_t landing_ahead;  /* his flight path (velocity, result.gravity down) meets a floor before a
                               wall; only needed during wall jumps */
    uint8_t has_wall_distance; /* wall_distance is valid */
    float wall_distance;    /* his position to the wall's surface along its normal, metres: then he
                               sticks from up to 0.775 m (x scale) out and is held 0.6 m out, as in
                               the game; without it he's pressed to a wall he touches */
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
    RATCHET_STATE_WALL_STICK = 10, /* stuck to a wall, then sliding down */
    RATCHET_STATE_WALL_JUMP = 11,  /* the jump off it */
    RATCHET_STATE_LEDGE_HANG = 12,  /* hanging from a ledge, or moving onto one */
    RATCHET_STATE_LEDGE_CLIMB = 13, /* climbing up a caught ledge */
    RATCHET_STATE_LEDGE_DROP = 14,  /* letting go of a placed ledge with a turn */
    RATCHET_STATE_LEDGE_JUMP = 15,  /* the jump up over a placed ledge */
    RATCHET_STATE_SWING = 16,       /* swinging on a Swingshot target */
    RATCHET_STATE_HOOKSHOT = 17,    /* reeled in to a Hookshot target */
    RATCHET_STATE_SWING_JUMP = 18,  /* the jump off a swing */
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
    RATCHET_EVENT_WALL_STUCK = 1u << 12,   /* stuck to a wall */
    RATCHET_EVENT_WALL_JUMPED = 1u << 13,  /* jumped off it */
    RATCHET_EVENT_LEDGE_GRABBED = 1u << 14, /* grabbed a ledge (result.ledge_caught: one found on the way down) */
    RATCHET_EVENT_LEDGE_CLIMBED = 1u << 15,
    RATCHET_EVENT_LEDGE_DROPPED = 1u << 16,
    RATCHET_EVENT_LEDGE_TRANSFERRED = 1u << 17, /* moving to another placed ledge */
    RATCHET_EVENT_LEDGE_JUMPED = 1u << 18,
    RATCHET_EVENT_SWING_ATTACHED = 1u << 19, /* fired the Swingshot (result.swing_hook: at a Hookshot target) */
    RATCHET_EVENT_SWING_RELEASED = 1u << 20, /* let go of the swing or the hook */
    RATCHET_EVENT_SWING_JUMPED = 1u << 21,
    RATCHET_EVENT_HOOK_ARRIVED = 1u << 22,   /* reeled all the way in */
};

typedef struct RatchetStepResult {
    RatchetVec3 velocity;    /* move by velocity * dt */
    uint32_t state;          /* RATCHET_STATE_* */
    uint32_t events;         /* RATCHET_EVENT_* bits */
    float facing_yaw;        /* radians, atan2(x, z) */
    float landed_fall_speed; /* valid when RATCHET_EVENT_LANDED is set */
    uint32_t melee_swing;    /* combo swing (0 = first) in progress or of this tick's swing/hit event */
    float wall_normal_x, wall_normal_z; /* on a wall or in a wall jump (10, 11): the wall's normal */
    uint8_t wall_right;      /* on a wall or in a wall jump: 1 if the game's right wall clips fit, 0 left */
    float gravity;           /* how fast he's pulled down now, m/s^2, e.g. for RatchetContact.landing_ahead */
    uint8_t ledge_caught;    /* with RATCHET_EVENT_LEDGE_GRABBED: 1 caught on the way down, 0 placed */
    uint8_t swing_hook;      /* with RATCHET_EVENT_SWING_ATTACHED: 1 if it was a Hookshot target */
    uint8_t has_swing_target; /* swing_target is set: the target he'd fire at now, or the one he's on
                                 (states 16 and 17), for a reticle and the cable */
    RatchetVec3 swing_target;
    uint8_t ledge_move;      /* the move onto a ledge (state 12): 0 none (hanging), 1 grabbing a placed
                                ledge, 2 onto a caught one, 3 up to another ledge, 4 down to one */
    uint8_t has_ledge;       /* on a ledge or moving to one: the point he holds on its edge, and the
                                way the wall below it faces (x, z) */
    RatchetVec3 ledge_point;
    float ledge_normal_x, ledge_normal_z;
    uint8_t swing_phase;     /* on a target (16, 17): 0 swinging in, 1 past it (Hookshot: reeled in),
                                2 from the first swing back */
    float swing_axis_x, swing_axis_z; /* on a swing: its line, the way he came in; the game's camera
                                         turns to look along it */
    float dash_dir_x, dash_dir_z;     /* the dash's direction in states 5 and 6 (after them, the last) */
    float swing_lean, swing_speed;    /* on a swing (16): the game's swing-loop animation values. Lean 0
                                         swinging ahead, 0.5 stalled at an end, 1 back; Speed toward 1
                                         as he passes slowly under the target */
} RatchetStepResult;

/* A placed ledge (the game's ledge paths): a straight stretch of edge he can hang from. */
typedef struct RatchetLedge {
    RatchetVec3 a, b;         /* the edge, end to end */
    float normal_x, normal_z; /* out of the wall below it, on the ground plane */
    uint32_t id;
    uint8_t grab, jump, drop, transfer; /* what it allows (0/1) */
} RatchetLedge;

/* A Swingshot target. */
typedef struct RatchetSwingTarget {
    RatchetVec3 position;
    uint8_t kind;             /* 0 he swings on it, 1 the Hookshot reels him in to it */
    float axis_x, axis_z;     /* its own forward on the ground plane: he swings in the upright plane along it */
    float length;             /* the cable's length on it (a Hookshot target at <= 0 takes the game's) */
    float min_range, max_range; /* its own range limits; <= 0 for none */
    uint32_t id;
} RatchetSwingTarget;

/* The host's level, as the sim asks about it. Any callback may be null. */
typedef struct RatchetWorld {
    void *user;
    /* Write up to max (32) placed ledges with any part within radius of center to out, nearest
       first; return how many. */
    uint32_t (*ledges)(void *user, RatchetVec3 center, float radius, RatchetLedge *out, uint32_t max);
    /* The first surface the ray from..to hits: return 1 and fill point and normal, or 0. */
    /* Null means no ledges to catch, and the Swingshot sees every target. */
    uint8_t (*raycast)(void *user, RatchetVec3 from, RatchetVec3 to, RatchetVec3 *point, RatchetVec3 *normal);
    /* Write up to max (32) Swingshot targets within radius of center to out, nearest first; return
       how many. Ids must be unique per target. */
    uint32_t (*swing_targets)(void *user, RatchetVec3 center, float radius, RatchetSwingTarget *out, uint32_t max);
} RatchetWorld;

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
RatchetSim *ratchet_new(float scale); /* scale <= 0 or not a number: 1 */
void ratchet_free(RatchetSim *sim);

/* Load extracted Rift Apart config JSON from dir (UTF-8). 0 = ok; on error the old tuning stays. */
int32_t ratchet_load_configs(RatchetSim *sim, const char *dir, float scale);
void ratchet_set_scale(RatchetSim *sim, float scale);

RatchetStepResult ratchet_step(RatchetSim *sim, float dt, RatchetInput input, RatchetContact contact, RatchetVec3 feet);
void ratchet_set_velocity(RatchetSim *sim, RatchetVec3 velocity);
/* Ledges and the Swingshot need the host's level; the callbacks must stay valid while the sim steps. */
void ratchet_set_world(RatchetSim *sim, RatchetWorld world);
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

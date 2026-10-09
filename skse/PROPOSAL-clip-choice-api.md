# Proposal: one clip picker for the sandbox and the SKSE plugin

For the session that owns `crates/` and `apps/`. Nothing here is built. It's a request from the
`skse` branch, which doesn't touch those folders.

**Why:** the owner asked whether the Skyrim port recreates the Rust demo. The movement does run in
Skyrim as Rust (`ratchet.dll`). The choice of which of Ratchet's clips plays is a hand port in C++
(`skse/src/Model.cpp`, `PickClip`), and it already differs from the sandbox's
(`apps/sandbox/src/ratchet_model.rs`, `play_clips`) in about ten places. The fix: move the movement
half of `play_clips` out of Bevy into a Rust crate. Expose it through `ratchet.dll`, and have the
sandbox and the plugin both call it. The plugin then only loads, samples and blends clips.

## 1. What moves out of `ratchet_model.rs`
**Pure clip choice** (glam types and the frame's dt only):
- names and rules: `CLIP_STATES`, dash sets, `LEAN_CLIPS`, strafe sets, glide lean, walk/run
  thresholds, fidgets, `MELEE_CLIPS`, swing loop, wall-jump rate, `LOOPING`, `SPEED_MATCHED`,
  `carried()`;
- the per-frame state kept in `Rig` (timers, previous state, walking, leans);
- the movement half of `play_clips`:
  - picks: respawn reset, landings, the 180° ledge catch, let-go and land timers, dash,
    strafe-jump and strafe-landing picks, glide intro then loop, wall stick then slide, swing
    entry, the `want` match and its fallbacks;
  - timing: fade times, looping, playback rates (speed-matched 0.5-1.5×, wall jump 0.25×, the
    swing jump timed to its `JumpPeak`), walk hysteresis, leans, glide lean, fidgets;
- helpers: `lean_into_turns`, `swing_blend_ms`, `swing_loop_weights`, `blend_swing_loop`, and
  the existing tests.

**Its inputs:**
- **from the sim:** state, velocity, facing, `ledge_move`, `dash_dir`, `melee_swing`,
  `wall_right`, `swing_phase`, swing lean and speed, and tuning (run speeds, scale);
- **from the host:** dt, ground distance, the respawn and ledge-drop counters, the weapon grip,
  aiming or firing ("strafing"), and whether the wrench is thrown;
- **from the clips:** which clips exist, each clip's length and root distance, and the swing
  jump's `JumpPeak` time.

**Its output** is a list of layers. One is the picked move clip, with a 0-500 ms crossfade, a
looping flag and a rate. Then the extra base clips: the swing loop's five, and two run leans. Then
one additive clip, the glide lean. There are no model offsets or mirroring: the model faces the
sim's yaw, and walls have their own left and right clips.

**Stays in the sandbox:** asset and graph setup, clip sounds, the procedural pose, and the
weapon layers (fire, catch, grenade, windup, aim offsets, throw, draw, glove hand).

## 2. Where the C++ copy differs today
- dashes are `dash` / `air_dash` only (the sandbox has eight directions);
- glide: no intro, loop or lean;
- the wall slide starts on a falling speed, not when the stick clip has finished;
- the wall jump plays at 1× (sandbox 0.25×);
- ledges: no 180° catches, `ledge_grab`, `ledge_up`, `ledge_down_to`, `ledge_drop_turn`, or
  let-go clip;
- no fidgets or run leans;
- fades of 90 or 160 ms with smoothstep (sandbox: linear, 0-500 ms);
- speed matching lacks the "root moves more than 0.5 m" filter and the run_max fallback;
- landings come from its own 0.15 s air timer, not the sim's state.

## 3. The proposal
**Where:**
- a new crate `crates/libratchet-anim` (MIT; depends on `libratchet` and `glam`);
- `libratchet-ffi` adds `src/anim.rs` on top of it, so there's still one `ratchet.dll`;
- the sandbox depends on the crate and deletes its own copy.

**Rust API:**
```rust
pub struct ClipInfo<'a> { pub name: &'a str, pub seconds: f32, pub root_distance: f32, pub jump_peak: Option<f32> }
#[derive(Clone, Copy, Default, PartialEq, Eq)] pub enum Grip { #[default] None, Pistol, Glove }
pub struct AnimInput {
    pub dt: f32,
    pub velocity: Vec3,               // how he really moves: m/s, sim axes, sim scale
    pub ground_distance: Option<f32>, // m, sim scale
    pub ledge_dropped: bool,          // a LedgeDropped in any sim step since the last update
    pub grip: Grip,
    pub strafing: bool,               // aiming, or a gun firing and just after
    pub throwing: bool,               // the wrench is out of his hand, until just after the catch
}
#[derive(Clone, Copy)] pub struct Layer { pub clip: usize, pub time: f32, pub weight: f32, pub additive: bool }
impl Animator {
    pub fn new(clips: &[ClipInfo]) -> Self;
    pub fn reset(&mut self); // respawn, load, teleport
    pub fn update(&mut self, sim: &Ratchet, input: &AnimInput) -> &[Layer]; // [0] = the picked move clip
}
```
- `pick.rs`: a straight port of section 1, in the same order.
- `mixer.rs`: every clip's clock and weight, as Bevy 0.19's `AnimationTransitions` does it:
  - a new pick restarts at 0, and the old one fades linearly;
  - the newest fading clip takes its share first; weights sum to 1;
  - looping clips wrap, one-shots hold, "finished" = a one-shot past its length;
  - a 0 ms fade is a cut.
- The swing loop's five clips share one virtual clip's weight; the run leans follow the run.
- Clips are identified by index in the host's table, matched by name once in `new`. A missing
  clip falls back as the sandbox does now.

**C API** (appended to `ratchet.h`; `RATCHET_API_VERSION` 1 → 2):
```c
typedef struct RatchetAnim RatchetAnim;
typedef struct RatchetAnimClip {
    const char *name;    /* the converter's clip name, UTF-8 ("run", "wall_stick_left"); copied */
    float seconds;       /* its length */
    float root_distance; /* metres its root travels over it at scale 1, 0 if none (feet-planted rates) */
    float jump_peak;     /* seconds into it of its JumpPeak event, < 0 if none (the swing jump) */
} RatchetAnimClip;
enum { RATCHET_GRIP_NONE = 0, RATCHET_GRIP_PISTOL = 1, RATCHET_GRIP_GLOVE = 2 };
typedef struct RatchetAnimInput {
    float dt;               /* seconds since the last update */
    RatchetVec3 velocity;   /* how he really moves, m/s, sim axes (Skyrim: the controller's) */
    uint8_t has_ground_distance;
    float ground_distance;  /* feet to ground, m */
    uint32_t events;        /* RATCHET_EVENT_* of every ratchet_step since the last update, or'ed */
    uint8_t grip;           /* RATCHET_GRIP_* */
    uint8_t strafing;       /* aiming or firing: the strafe clips */
    uint8_t throwing;       /* the wrench is out of his hand: the melee grip */
} RatchetAnimInput;
typedef struct RatchetAnimLayer {
    uint32_t clip;          /* index into the clips given to ratchet_anim_new */
    float time;             /* seconds into it (loops wrapped, one-shots held): sample without looping */
    float weight;           /* base layers: shares of the pose, summing to 1; additive: 0..1 */
    uint8_t additive;       /* 1: add on top of the blended base pose (the glide's lean) */
} RatchetAnimLayer;
#define RATCHET_ANIM_MAX_LAYERS 16
RatchetAnim *ratchet_anim_new(const RatchetAnimClip *clips, uint32_t count); /* free with ratchet_anim_free */
void ratchet_anim_free(RatchetAnim *anim);
void ratchet_anim_reset(RatchetAnim *anim);
/* Once a frame, after that frame's ratchet_step calls. Writes up to max layers to out, the picked
   move clip first; returns how many. */
uint32_t ratchet_anim_update(RatchetAnim *anim, const RatchetSim *sim, RatchetAnimInput input,
                             RatchetAnimLayer *out, uint32_t max);
```
Ownership is like `RatchetSim`: the host creates and frees it, names are copied, `out` is the
host's, the sim is only read during the call, and null pointers return 0. **Blending contract**
(what Bevy's blend node does): base layers in order, each moving the pose toward itself by its
weight over the sum so far. Additive layers add w × translation and rotate by
slerp(identity, r, w) applied before the base rotation.

**The sandbox:** builds the table from its glTF animations, then drives one graph node per layer
each frame (speed 0, `seek_to`, `set_weight`, as its aim and glide-lean layers already do).
Respawn calls `reset()`. Weapon layers and sounds stay.

**The SKSE plugin:** builds the table from `Anim::Library` (jump_peak = -1: RANM has no events),
passes the controller's velocity (on the ground Skyrim moves him), the ground distance, the step
events and the grip, and samples and blends the layers. `PickClip`, `Play`, `Looping` and the
two-slot fade go.

## 4. Risks and open questions
1. **Drift is happening now:** the C++ wall and ledge picks already differ. Freeze `PickClip`
   until the crate lands.
2. **Ground flicker in Skyrim:** on stairs Skyrim's controller is in the air for single frames,
   and the sim falls at once. Snap small drops in the contact the plugin sends, as the sandbox
   does.
3. **Velocity must be the real movement** (Skyrim's locomotion on the ground), or the feet slide.
4. **Hidden dependency:** turn rate and `prev_yaw` are only measured inside `lean_into_turns`,
   which returns early without the lean clips. The glide lean and the 180° catch depend on them,
   so the port should track the yaw regardless.
5. Two one-frame lags in the sandbox (the swing-loop weights, the run leans' time) go away,
   which changes the sandbox slightly.
6. **No grip in the sandbox:** it always has the pistol out, so it never shows the empty-handed
   set. Decide what "no grip" glides with, and add an empty-handed slot to compare like for like.
7. The plugin's clip file has had no weapon clips; it gets them from now on, without clip events.
8. RANM samples resampled keys with nlerp; Bevy samples the glTF curves with slerp. Clips, times
   and weights would match; poses in between can differ a little.
9. The API version bump needs `ratchet.dll` and the plugin rebuilt together (the plugin refuses
   a mismatched DLL).

## Second request: C bindings for `libarsenal` (the guns)
The Skyrim plugin now fights. Its wrench uses the existing C API. Its gun can't: `libarsenal`
(`Weapon::step`, `Projectile::step`, `WeaponParams::from_riftapart_config`) is Rust-only, so the
plugin has a small C++ stand-in for the Burst Pistol's trigger (`skse/src/Combat.cpp`: fire rates,
spread growth and recovery, from the sandbox's reading of `wpn_burstpistol_weapon.json`). That's
the same duplication as the clip choice. A small C API would end it, again in `ratchet.dll`:

```c
typedef struct RatchetWeapon RatchetWeapon;
/* A weapon from the game's config JSON (e.g. wpn_burstpistol_weapon.json), UTF-8 path; null on error. */
RatchetWeapon *ratchet_weapon_load(const char *config_path, float scale);
void ratchet_weapon_free(RatchetWeapon *weapon);
typedef struct RatchetShot { RatchetVec3 origin, velocity; } RatchetShot;
/* One tick: the trigger (0..1, so a gamepad's half press can be told apart), the muzzle, the point
   aimed at, aimed = the aim button is held. Writes up to max shots fired this tick; returns how many. */
uint32_t ratchet_weapon_step(RatchetWeapon *weapon, float dt, float trigger, RatchetVec3 muzzle,
                             RatchetVec3 target, uint8_t aimed, RatchetShot *out, uint32_t max);
/* Its numbers for the host's hits: damage per shot, projectile radius, range, ammo left (-1 unlimited). */
typedef struct RatchetWeaponInfo { float damage, radius, range; int32_t ammo; } RatchetWeaponInfo;
void ratchet_weapon_info(const RatchetWeapon *weapon, RatchetWeaponInfo *out);
```

The host keeps flying and hitting with its own engine (Skyrim launches a projectile per shot).
`Projectile::step` could come later for hosts without projectiles. The same version bump as above
covers it.

**Update (2026-10-09):** the plugin now has the Shatterbomb too, as a second C++ stand-in: the
cooked trigger, `lob_velocity`, the flight under gravity and `ExplosionParams::damage_at`, with the
numbers from `wpn_protogrenade_weapon.json` written into `skse/src/Combat.cpp`. So the API above
also wants:
- a way to tell a cooked shot's wind-up (`Weapon::cooking`), for the wind-up pose;
- the lob solved at release from where the hand is then (the sandbox's `last_grenade_shot`);
- the explosion's numbers in `RatchetWeaponInfo` (inner and outer radius and damage, knockback
  tiers), or a `ratchet_weapon_blast_damage(weapon, distance)`;
- and, for hosts that fly the bomb themselves, `Projectile::step` after all.

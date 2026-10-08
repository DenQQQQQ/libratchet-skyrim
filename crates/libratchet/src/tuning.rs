//! Tuning values for the movement sim.
//!
//! `Tuning::default()` holds generic values written for this project. They're close to how a
//! Ratchet & Clank game feels but aren't copied from one. `Tuning::from_riftapart_configs` fills
//! the same struct from JSON that the user extracts from their own Rift Apart install (see
//! `tools/extract_riftapart_configs.py`). No game data ships with this crate.
//!
//! Units: metres, seconds, metres per second.

use serde_json::Value;
use std::path::Path;

/// One jump "kind" (single, double, fall, glide...), mirroring how Rift Apart's
/// `HeroTraversalConfig.JumpConfigs` entries are shaped.
#[derive(Clone, Debug, PartialEq)]
pub struct JumpParams {
    /// Apex height for a tap, in metres above the take-off point.
    pub height_min: f32,
    /// Apex height when the button is held for `button_max_hold` or longer.
    pub height_max: f32,
    /// Seconds to reach `height_max`, or the gravity itself (m/s²) when
    /// `time_to_peak_is_gravity` is set.
    pub time_to_peak: f32,
    pub time_to_peak_is_gravity: bool,
    /// Hold time at which the height starts growing past `height_min`.
    pub button_min_hold: f32,
    /// Hold time at which the height reaches `height_max`.
    pub button_max_hold: f32,
    /// Fastest downward speed while this jump kind is active.
    pub terminal_velocity: f32,
    /// Seconds after this jump starts before holding jump can turn into a glide.
    pub no_glide_time: f32,
    /// Seconds after this jump starts before a mid-air jump is allowed.
    pub double_jump_time_start: f32,
    /// Height above the take-off point (or above the ground, when falling) needed before a
    /// mid-air jump or glide is allowed.
    pub double_jump_height_min: f32,
    /// Cap on the combined height of a jump chain, measured from the first take-off.
    pub max_total_jump: Option<f32>,
    /// How much of the speed he had a jump keeps each tick (RiftApart.exe, see
    /// [`JumpParams::momentum_keep`]): the weight goes from MomentumWeightMin to Max over
    /// MomentumMinTime + MomentumTransitionTime.
    pub momentum_weight_min: f32,
    pub momentum_weight_max: f32,
    pub momentum_min_time: f32,
    pub momentum_transition_time: f32,
    /// Seconds after take-off before the stick steers the jump (until then it keeps going as it
    /// took off).
    pub no_input_time: f32,
}

impl JumpParams {
    /// Values used when a config leaves a field out. Rift Apart's configs omit fields that
    /// match the engine default; these are our best guesses at those defaults.
    pub const FALLBACK: JumpParams = JumpParams {
        height_min: 0.0,
        height_max: 0.0,
        time_to_peak: 0.0,
        time_to_peak_is_gravity: false,
        button_min_hold: 0.0,
        button_max_hold: 0.0,
        terminal_velocity: 50.0,
        no_glide_time: 0.0,
        double_jump_time_start: 0.0,
        double_jump_height_min: 0.0,
        max_total_jump: None,
        momentum_weight_min: 0.0,
        momentum_weight_max: 0.0,
        momentum_min_time: 0.0,
        momentum_transition_time: 0.0,
        no_input_time: 0.0,
    };

    /// Gravity (m/s², positive = down) while this jump kind is active.
    pub fn gravity(&self) -> f32 {
        self.gravity_for(self.height_min)
    }

    /// Gravity for a jump whose (possibly capped) tap height is `height_min`. Rift Apart derives
    /// it from the *tap* height, `2·HeightMin / TimeToPeak²`, so a tap peaks exactly at
    /// `TimeToPeak` and holding for more height takes longer (RiftApart.exe).
    pub fn gravity_for(&self, height_min: f32) -> f32 {
        if self.time_to_peak_is_gravity {
            self.time_to_peak
        } else if self.time_to_peak > 0.0 && height_min > 0.0 {
            2.0 * height_min / (self.time_to_peak * self.time_to_peak)
        } else {
            25.0
        }
    }

    /// How far (0..=1) a button held `held` seconds moves the apex from `height_min` towards
    /// `height_max` (RiftApart.exe, including its equal-times special case).
    pub fn hold_fraction(&self, held: f32) -> f32 {
        let span = self.button_max_hold - self.button_min_hold;
        if span.abs() <= 1e-6 {
            return if held > self.button_min_hold {
                1.0
            } else if held == self.button_min_hold {
                0.5
            } else {
                0.0
            };
        }
        ((held - self.button_min_hold) / span).clamp(0.0, 1.0)
    }

    /// The share of the old horizontal speed a jump keeps over a tick of `dt`, `t` seconds after
    /// take-off (RiftApart.exe): the momentum weight raised to half the tick over the time to
    /// peak, mapped back onto the weights' range. A single jump (1 to 1.2) keeps about 0.03 a
    /// tick at 60 Hz, so the stick decides almost at once; a double jump (0 to 1.4) about 0.72.
    pub fn momentum_keep(&self, t: f32, dt: f32) -> f32 {
        let (lo, hi) = (self.momentum_weight_min, self.momentum_weight_max);
        // (Over MinTime + TransitionTime, as the game divides.)
        let span = self.momentum_min_time + self.momentum_transition_time;
        let s = if span >= 1e-4 { ((t - self.momentum_min_time) / span).clamp(0.0, 1.0) } else { 1.0 };
        let w = lo + s * (hi - lo);
        let peak_time = if self.time_to_peak_is_gravity {
            (2.0 * self.height_min / self.time_to_peak.max(1e-6)).sqrt()
        } else {
            self.time_to_peak
        };
        let f = w.max(0.0).powf(0.5 * dt / if peak_time > 0.0 { peak_time } else { 1.0 });
        if (hi - lo).abs() < 1e-6 {
            // The game divides by zero here; IEEE rules then clamp to 0 or 1.
            return if f > lo { 1.0 } else { 0.0 };
        }
        ((f - lo) / (hi - lo)).clamp(0.0, 1.0)
    }

    /// Apex height for a button held `held` seconds.
    pub fn height_for_hold(&self, held: f32) -> f32 {
        self.height_min + (self.height_max - self.height_min) * self.hold_fraction(held)
    }

    fn scaled(&self, s: f32) -> JumpParams {
        JumpParams {
            height_min: self.height_min * s,
            height_max: self.height_max * s,
            time_to_peak: if self.time_to_peak_is_gravity { self.time_to_peak * s } else { self.time_to_peak },
            terminal_velocity: self.terminal_velocity * s,
            double_jump_height_min: self.double_jump_height_min * s,
            max_total_jump: self.max_total_jump.map(|h| h * s),
            ..self.clone()
        }
    }
}

/// Speeds for one movement mode, shaped like Rift Apart's speed configs: a walk band below
/// the stick threshold and a run band above it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeedParams {
    pub walk_min: f32,
    pub walk_max: f32,
    pub run_min: f32,
    pub run_max: f32,
    /// Stick magnitude (0..1) where walking turns into running.
    pub walk_run_threshold: f32,
}

impl SpeedParams {
    /// Target speed for a stick magnitude in 0..=1.
    pub fn speed_for_stick(&self, m: f32) -> f32 {
        let m = m.clamp(0.0, 1.0);
        if m <= 1e-3 {
            return 0.0;
        }
        let th = self.walk_run_threshold.clamp(0.0, 0.999);
        if m < th {
            lerp(self.walk_min, self.walk_max, m / th)
        } else {
            lerp(self.run_min, self.run_max, (m - th) / (1.0 - th))
        }
    }

    fn scaled(&self, s: f32) -> SpeedParams {
        SpeedParams {
            walk_min: self.walk_min * s,
            walk_max: self.walk_max * s,
            run_min: self.run_min * s,
            run_max: self.run_max * s,
            walk_run_threshold: self.walk_run_threshold,
        }
    }
}

/// Sticking to a wall that takes wall jumps (the game's WallStickConfig).
#[derive(Clone, Debug, PartialEq)]
pub struct WallStickParams {
    /// Seconds he holds still on the wall before sliding down it.
    pub no_slide_time: f32,
    /// Sliding down: acceleration (m/s², down) and top speed.
    pub slide_gravity: f32,
    pub slide_terminal: f32,
    /// He sticks only at least this high above the ground.
    pub min_ground_height: f32,
    /// On a plain wall, he sticks only facing (or pushing the stick) within this many degrees of
    /// straight into it.
    pub facing_threshold: f32,
    /// Jumping up a plain wall (a double jump off it) leaves him at least this much height.
    pub up_min_height_gain: f32,
}

impl WallStickParams {
    fn scaled(&self, s: f32) -> WallStickParams {
        WallStickParams {
            slide_gravity: self.slide_gravity * s,
            slide_terminal: self.slide_terminal * s,
            min_ground_height: self.min_ground_height * s,
            up_min_height_gain: self.up_min_height_gain * s,
            ..self.clone()
        }
    }
}

/// A dash: speed up to `max_speed` at `accel`, and once `distance` is covered slow down at
/// `decel`. Shaped like Rift Apart's DodgeConfig / AirDashConfig.
#[derive(Clone, Debug, PartialEq)]
pub struct DashParams {
    pub distance: f32,
    pub max_speed: f32,
    /// m/s²; 0 means the dash starts at full speed.
    pub accel: f32,
    /// m/s²; 0 means the dash stops at the end of `distance`.
    pub decel: f32,
}

impl DashParams {
    fn scaled(&self, s: f32) -> DashParams {
        DashParams { distance: self.distance * s, max_speed: self.max_speed * s, accel: self.accel * s, decel: self.decel * s }
    }
}

/// One swing of the wrench combo, shaped by its animation in the game: how long it lasts, when
/// the wrench connects, when other moves may cut it short, and the forward lunge (the clip's
/// root motion).
#[derive(Clone, Debug, PartialEq)]
pub struct SwingParams {
    pub duration: f32,
    /// Seconds into the swing when the wrench connects (the clip's MeleeDamageImpact event).
    pub hit_time: f32,
    /// The clip's two AllowEarlyTransition events, which RiftApart.exe's melee state reads:
    /// from `early_exit` the next swing may start and the stick may cut the swing short; at
    /// `exit` it's over.
    pub early_exit: f32,
    pub exit: f32,
    /// Forward distance from where the swing started, one sample every `1 / lunge_rate` s.
    /// Empty: no lunge.
    pub lunge: Vec<f32>,
    pub lunge_rate: f32,
    /// For hosts: damage, and how far it knocks a standard enemy back.
    pub damage: f32,
    pub knockback: f32,
}

impl SwingParams {
    /// Forward distance covered `t` seconds into the swing.
    pub fn lunge_at(&self, t: f32) -> f32 {
        let Some(&last) = self.lunge.last() else { return 0.0 };
        let x = (t * self.lunge_rate).max(0.0);
        let i = x.floor() as usize;
        if i + 1 >= self.lunge.len() {
            return last;
        }
        lerp(self.lunge[i], self.lunge[i + 1], x - i as f32)
    }

    /// A swing that lunges `distance` at an even speed over its first `lunge_time` seconds and
    /// plays out in full (no early exit).
    fn even(duration: f32, hit_time: f32, distance: f32, lunge_time: f32, damage: f32, knockback: f32) -> SwingParams {
        let rate = 30.0;
        let n = ((lunge_time * rate).ceil() as usize).max(1);
        SwingParams {
            duration,
            hit_time,
            early_exit: duration,
            exit: duration,
            lunge: (0..=n).map(|k| distance * k as f32 / n as f32).collect(),
            lunge_rate: rate,
            damage,
            knockback,
        }
    }

    fn scaled(&self, s: f32) -> SwingParams {
        SwingParams { lunge: self.lunge.iter().map(|d| d * s).collect(), knockback: self.knockback * s, ..self.clone() }
    }
}

/// The OmniWrench throw: out along the aim, then back to the hand. Shaped like Rift Apart's
/// `WeaponWrenchPrius`.
#[derive(Clone, Debug, PartialEq)]
pub struct ThrowParams {
    pub out_speed: f32,
    pub max_distance: f32,
    pub return_max_speed: f32,
    /// How quickly the return speeds up: its acceleration grows by this much per second (m/s³).
    pub return_jerk: f32,
    /// Degrees per second.
    pub spin_speed: f32,
    /// For hosts.
    pub damage: f32,
    pub damage_radius: f32,
    /// The wrench is caught once it's this close to the hand (ours, not from the game).
    pub catch_radius: f32,
    /// Seconds from the start of the throw animation until the wrench leaves the hand (the
    /// clip's WeaponThrown event).
    pub release: f32,
}

impl ThrowParams {
    fn scaled(&self, s: f32) -> ThrowParams {
        ThrowParams {
            out_speed: self.out_speed * s,
            max_distance: self.max_distance * s,
            return_max_speed: self.return_max_speed * s,
            return_jerk: self.return_jerk * s,
            damage_radius: self.damage_radius * s,
            catch_radius: self.catch_radius * s,
            ..self.clone()
        }
    }
}

/// OmniWrench melee: the ground combo and the Hyper-Strike (melee in the air: a hop, then a
/// slam straight down).
#[derive(Clone, Debug, PartialEq)]
pub struct MeleeParams {
    /// The ground combo in order. Pressing on after the last swing starts it over.
    pub combo: Vec<SwingParams>,
    /// A press this long before Ratchet can swing still counts (RiftApart.exe: the melee
    /// component looks back up to 0.25 s).
    pub press_buffer: f32,
    /// After a swing (not the last) ends or is cut short, a press within this long goes on
    /// with the combo; later presses start it over (RiftApart.exe: 0.35 s).
    pub combo_timeout: f32,
    /// The combo's rest: presses are ignored this long from the last swing's first
    /// AllowEarlyTransition, again from its second (RiftApart.exe: 0.4 s).
    pub combo_cooldown: f32,
    /// Hyper-Strike: melee in the air hops like this jump...
    pub smash_hop: JumpParams,
    /// ...when at least this far above the ground...
    pub smash_min_height: f32,
    /// ...then slams down at this gravity (m/s²), no faster than the hop's terminal velocity.
    pub smash_gravity: f32,
    /// Forward drift during the hop (the intro animation's root motion).
    pub smash_intro: SwingParams,
    /// On the ground after the slam: from `smash_land_early_exit` a jump, a swing or the
    /// stick cut it short; at `smash_land_time` it's over (the landing clip's
    /// AllowEarlyTransition events).
    pub smash_land_early_exit: f32,
    pub smash_land_time: f32,
    /// For hosts: the slam's damage and radius.
    pub smash_damage: f32,
    pub smash_radius: f32,
    pub throw: ThrowParams,
}

impl Default for MeleeParams {
    fn default() -> Self {
        MeleeParams {
            // Ours, roughly shaped like the game's clips (they load from hero_melee_clips.json).
            combo: vec![
                SwingParams { early_exit: 0.2, exit: 0.5, ..SwingParams::even(0.9, 0.1, 0.6, 0.15, 10.0, 2.0) },
                SwingParams { early_exit: 0.25, exit: 0.45, ..SwingParams::even(0.6, 0.12, 0.8, 0.25, 10.0, 2.0) },
                SwingParams { early_exit: 0.55, exit: 0.85, ..SwingParams::even(0.95, 0.3, 1.2, 0.45, 15.0, 3.5) },
            ],
            press_buffer: 0.25,
            combo_timeout: 0.35,
            combo_cooldown: 0.4,
            smash_hop: JumpParams {
                height_min: 0.8,
                height_max: 0.8,
                time_to_peak: 0.25,
                terminal_velocity: 60.0,
                ..JumpParams::FALLBACK
            },
            smash_min_height: 0.4,
            smash_gravity: 800.0,
            smash_intro: SwingParams::even(0.45, 0.0, 1.0, 0.4, 0.0, 0.0),
            smash_land_early_exit: 0.2,
            smash_land_time: 0.4,
            smash_damage: 25.0,
            smash_radius: 2.5,
            throw: ThrowParams {
                out_speed: 25.0,
                max_distance: 14.0,
                return_max_speed: 50.0,
                return_jerk: 60.0,
                spin_speed: 1500.0,
                damage: 15.0,
                damage_radius: 0.5,
                catch_radius: 0.6,
                release: 0.1,
            },
        }
    }
}

impl MeleeParams {
    fn scaled(&self, s: f32) -> MeleeParams {
        MeleeParams {
            combo: self.combo.iter().map(|w| w.scaled(s)).collect(),
            smash_hop: self.smash_hop.scaled(s),
            smash_min_height: self.smash_min_height * s,
            smash_gravity: self.smash_gravity * s,
            smash_intro: self.smash_intro.scaled(s),
            smash_radius: self.smash_radius * s,
            throw: self.throw.scaled(s),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tuning {
    pub ground_speeds: SpeedParams,
    pub air_speeds: SpeedParams,
    pub glide_speeds: SpeedParams,
    /// Not found in the configs yet; tuned by feel (m/s²).
    pub ground_accel: f32,
    pub ground_decel: f32,
    pub air_accel: f32,
    pub glide_accel: f32,
    /// How fast the character turns in the air and during moves (rad/s, ours). On the ground
    /// the game's mover turn applies (see `Ratchet`).
    pub turn_rate: f32,
    pub single_jump: JumpParams,
    pub double_jump: JumpParams,
    /// Walking off a ledge without jumping.
    pub fall: JumpParams,
    pub glide: JumpParams,
    pub max_midair_jumps: u32,
    /// Phantom Dash on the ground.
    pub ground_dash: DashParams,
    /// Dash in the air (once per air time).
    pub air_dash: DashParams,
    /// OmniWrench combo, Hyper-Strike and throw.
    pub melee: MeleeParams,
    /// Aim mode: strafing speeds (forward run/walk, and backing up) and the lower,
    /// fixed-height jumps. Strafing while firing from the hip keeps the normal speeds.
    pub aim_speeds: SpeedParams,
    pub aim_back_speed: f32,
    pub aim_single_jump: JumpParams,
    pub aim_double_jump: JumpParams,
    /// The jump up off a ledge he hangs from, over its lip (the game's LedgeJumpConfig, with
    /// LedgeJumpSpeeds).
    pub ledge_jump: JumpParams,
    pub ledge_jump_speeds: SpeedParams,
    /// Wall jumps: sticking to the wall, the jump off it, and its speeds (the game's
    /// WallStickConfig, WallJumpConfig and WallJumpSpeeds, which the wall jump picks by its
    /// jump kind).
    pub wall_stick: WallStickParams,
    pub wall_jump: JumpParams,
    pub wall_jump_speeds: SpeedParams,
    /// Size multiplier applied by `scaled`; 1.0 means Ratchet-sized.
    pub scale: f32,
    /// Where these values came from, for display.
    pub source: String,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            ground_speeds: SpeedParams { walk_min: 1.2, walk_max: 1.6, run_min: 4.0, run_max: 5.5, walk_run_threshold: 0.6 },
            air_speeds: SpeedParams { walk_min: 1.0, walk_max: 3.0, run_min: 4.0, run_max: 6.0, walk_run_threshold: 0.6 },
            glide_speeds: SpeedParams { walk_min: 0.0, walk_max: 0.0, run_min: 0.0, run_max: 5.5, walk_run_threshold: 0.0 },
            ground_accel: 35.0,
            ground_decel: 45.0,
            air_accel: 18.0,
            glide_accel: 10.0,
            turn_rate: 14.0,
            single_jump: JumpParams {
                height_min: 1.0,
                height_max: 1.6,
                time_to_peak: 0.3,
                button_min_hold: 0.02,
                button_max_hold: 0.2,
                no_glide_time: 0.6,
                double_jump_time_start: 0.1,
                double_jump_height_min: 0.25,
                // The game's single-jump momentum weights.
                momentum_weight_min: 1.0,
                momentum_weight_max: 1.2,
                ..JumpParams::FALLBACK
            },
            double_jump: JumpParams {
                height_min: 1.4,
                height_max: 1.4,
                time_to_peak: 0.3,
                button_max_hold: 0.2,
                no_glide_time: 0.5,
                max_total_jump: Some(2.8),
                momentum_weight_max: 1.4,
                ..JumpParams::FALLBACK
            },
            fall: JumpParams {
                time_to_peak: 25.0,
                time_to_peak_is_gravity: true,
                no_glide_time: 0.1,
                double_jump_time_start: 0.1,
                double_jump_height_min: 0.25,
                ..JumpParams::FALLBACK
            },
            glide: JumpParams {
                time_to_peak: 10.0,
                time_to_peak_is_gravity: true,
                terminal_velocity: 4.0,
                double_jump_height_min: 1.3,
                ..JumpParams::FALLBACK
            },
            max_midair_jumps: 1,
            ground_dash: DashParams { distance: 6.5, max_speed: 28.0, accel: 100.0, decel: 50.0 },
            air_dash: DashParams { distance: 6.5, max_speed: 12.0, accel: 0.0, decel: 0.0 },
            melee: MeleeParams::default(),
            aim_speeds: SpeedParams { walk_min: 1.0, walk_max: 1.5, run_min: 4.0, run_max: 5.0, walk_run_threshold: 0.6 },
            aim_back_speed: 4.0,
            aim_single_jump: JumpParams {
                height_min: 0.9,
                height_max: 0.9,
                time_to_peak: 0.3,
                no_glide_time: 0.7,
                double_jump_time_start: 0.1,
                double_jump_height_min: 0.25,
                momentum_weight_min: 1.0,
                momentum_weight_max: 1.2,
                ..JumpParams::FALLBACK
            },
            aim_double_jump: JumpParams {
                height_min: 0.9,
                height_max: 0.9,
                time_to_peak: 0.28,
                no_glide_time: 0.5,
                max_total_jump: Some(1.7),
                momentum_weight_max: 1.4,
                ..JumpParams::FALLBACK
            },
            // The game's ledge jump (hero_traversal), as defaults.
            ledge_jump: JumpParams {
                height_min: 2.0,
                height_max: 2.5,
                time_to_peak: 0.35,
                button_min_hold: 0.02,
                button_max_hold: 0.2,
                terminal_velocity: 50.0,
                no_glide_time: 0.75,
                double_jump_time_start: 0.2,
                double_jump_height_min: 1.3,
                momentum_weight_max: 1.4,
                ..JumpParams::FALLBACK
            },
            ledge_jump_speeds: SpeedParams { walk_min: 1.0, walk_max: 1.0, run_min: 1.0, run_max: 2.0, walk_run_threshold: 0.98 },
            wall_jump_speeds: SpeedParams { walk_min: 5.0, walk_max: 5.0, run_min: 5.0, run_max: 5.0, walk_run_threshold: 0.5 },
            // The game's wall-jump values (hero_traversal), as defaults.
            // WallJumpUpMinHeightGain isn't in the asset; 0.5 is the exe's default for it.
            wall_stick: WallStickParams {
                no_slide_time: 0.3,
                slide_gravity: 20.0,
                slide_terminal: 40.0,
                min_ground_height: 1.0,
                facing_threshold: 50.0,
                up_min_height_gain: 0.5,
            },
            wall_jump: JumpParams {
                height_min: 2.18,
                height_max: 2.18,
                time_to_peak: 0.41,
                button_min_hold: 0.05,
                button_max_hold: 0.2,
                no_glide_time: 0.7,
                double_jump_time_start: 0.7,
                momentum_weight_max: 0.1,
                no_input_time: 0.7,
                ..JumpParams::FALLBACK
            },
            scale: 1.0,
            source: "generic defaults".into(),
        }
    }
}

impl Tuning {
    /// Builds tuning from the JSON files written by the extractor: `hero_traversal.json` and
    /// `hero_shared.json` in `dir`, plus the wrench files when present (`wpn_wrench_melee.json`,
    /// `wpn_wrench_weapon.json`, `hero_melee_clips.json`); melee falls back to the defaults.
    pub fn from_riftapart_configs(dir: &Path) -> Result<Tuning, String> {
        let traversal = read_json(&dir.join("hero_traversal.json"))?;
        let shared = read_json(&dir.join("hero_shared.json"))?;
        let jumps = &traversal["JumpConfigs"];
        if !jumps.is_object() {
            return Err("hero_traversal.json has no JumpConfigs".into());
        }
        let speeds = &shared["Motion"]["StandardSpeeds"];

        let base = Tuning::default();
        Ok(Tuning {
            ground_speeds: speeds_from(&speeds["NormalSpeedConfig"], &base.ground_speeds),
            air_speeds: speeds_from(&speeds["JumpSpeedConfig"], &base.air_speeds),
            glide_speeds: speeds_from(&traversal["GlideSpeeds"], &base.glide_speeds),
            single_jump: jump_from(&jumps["SingleJumpConfig"])?,
            double_jump: jump_from(&jumps["DoubleJumpConfig"])?,
            fall: jump_from(&jumps["FallJumpConfig"])?,
            glide: jump_from(&jumps["GlideJumpConfig"])?,
            max_midair_jumps: traversal["MaximumMidairJumps"].as_u64().unwrap_or(1) as u32,
            ground_dash: dash_from(&traversal["DodgeConfig"], &base.ground_dash),
            air_dash: dash_from(&traversal["AirDashConfig"], &base.air_dash),
            melee: melee_from(dir, &traversal, &shared, &base.melee),
            // The game's motion config reads only StandardSpeeds (and MagBootSpeeds); the other
            // speed blocks in hero_shared, StandAimSpeedConfig among them, are never read
            // (RiftApart.exe). Aim mode takes its AimSpeedConfig (our reading of the name).
            aim_speeds: speeds_from(&speeds["AimSpeedConfig"], &base.aim_speeds),
            aim_back_speed: f(&speeds["AimSpeedConfig"], "RunSpeedBackwardMax").unwrap_or(base.aim_back_speed),
            aim_single_jump: jump_from(&jumps["AimModeSingleJumpConfig"]).unwrap_or(base.aim_single_jump),
            aim_double_jump: jump_from(&jumps["AimModeDoubleJumpConfig"]).unwrap_or(base.aim_double_jump),
            ledge_jump: jump_from(&jumps["LedgeJumpConfig"]).unwrap_or(base.ledge_jump),
            ledge_jump_speeds: speeds_from(&traversal["LedgeJumpSpeeds"], &base.ledge_jump_speeds),
            wall_stick: wall_stick_from(&traversal["WallStickConfig"], &base.wall_stick),
            wall_jump: jump_from(&jumps["WallJumpConfig"]).unwrap_or(base.wall_jump),
            wall_jump_speeds: speeds_from(&traversal["WallJumpSpeeds"], &base.wall_jump_speeds),
            source: format!("Rift Apart configs ({})", dir.display()),
            ..base
        })
    }

    /// Returns a copy with every distance and speed multiplied by `s` (times stay the same),
    /// so the moves keep their timing on a bigger or smaller character.
    pub fn scaled(&self, s: f32) -> Tuning {
        Tuning {
            ground_speeds: self.ground_speeds.scaled(s),
            air_speeds: self.air_speeds.scaled(s),
            glide_speeds: self.glide_speeds.scaled(s),
            ground_accel: self.ground_accel * s,
            ground_decel: self.ground_decel * s,
            air_accel: self.air_accel * s,
            glide_accel: self.glide_accel * s,
            single_jump: self.single_jump.scaled(s),
            double_jump: self.double_jump.scaled(s),
            fall: self.fall.scaled(s),
            glide: self.glide.scaled(s),
            ground_dash: self.ground_dash.scaled(s),
            air_dash: self.air_dash.scaled(s),
            melee: self.melee.scaled(s),
            aim_speeds: self.aim_speeds.scaled(s),
            aim_back_speed: self.aim_back_speed * s,
            aim_single_jump: self.aim_single_jump.scaled(s),
            aim_double_jump: self.aim_double_jump.scaled(s),
            ledge_jump: self.ledge_jump.scaled(s),
            ledge_jump_speeds: self.ledge_jump_speeds.scaled(s),
            wall_stick: self.wall_stick.scaled(s),
            wall_jump: self.wall_jump.scaled(s),
            wall_jump_speeds: self.wall_jump_speeds.scaled(s),
            scale: self.scale * s,
            ..self.clone()
        }
    }
}

/// WallStickConfig: the gravity and terminal velocity are given as negative (down) speeds.
fn wall_stick_from(v: &Value, base: &WallStickParams) -> WallStickParams {
    if !v.is_object() {
        return base.clone();
    }
    WallStickParams {
        no_slide_time: f(v, "WallStickNoSlideTime").unwrap_or(base.no_slide_time),
        slide_gravity: f(v, "WallStickGravity").map(f32::abs).unwrap_or(base.slide_gravity),
        slide_terminal: f(v, "WallStickTerminalVelocity").map(f32::abs).unwrap_or(base.slide_terminal),
        min_ground_height: f(v, "WallStickMinGroundHeight").unwrap_or(base.min_ground_height),
        facing_threshold: f(v, "WallStickFacingIntoWallThreshold").unwrap_or(base.facing_threshold),
        up_min_height_gain: f(v, "WallJumpUpMinHeightGain").unwrap_or(base.up_min_height_gain),
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn f(v: &Value, key: &str) -> Option<f32> {
    v.get(key).and_then(Value::as_f64).map(|x| x as f32)
}

fn jump_from(v: &Value) -> Result<JumpParams, String> {
    if !v.is_object() {
        return Err("missing jump config".into());
    }
    let d = JumpParams::FALLBACK;
    Ok(JumpParams {
        height_min: f(v, "HeightMin").unwrap_or(d.height_min),
        height_max: f(v, "HeightMax").unwrap_or(d.height_max),
        time_to_peak: f(v, "TimeToPeak").unwrap_or(d.time_to_peak),
        time_to_peak_is_gravity: v.get("TimeToPeakIsGravity").and_then(Value::as_bool).unwrap_or(false),
        // -1 means "not used" in the configs.
        button_min_hold: f(v, "ButtonMinHoldTime").unwrap_or(d.button_min_hold).max(0.0),
        button_max_hold: f(v, "ButtonMaxHoldTime").unwrap_or(d.button_max_hold).max(0.0),
        terminal_velocity: f(v, "TerminalVelocity").unwrap_or(d.terminal_velocity),
        no_glide_time: f(v, "NoGlideTime").unwrap_or(d.no_glide_time),
        double_jump_time_start: f(v, "DoubleJumpTimeStart").unwrap_or(d.double_jump_time_start),
        double_jump_height_min: f(v, "DoubleJumpHeightMin").unwrap_or(d.double_jump_height_min),
        max_total_jump: f(v, "MaxTotalJump"),
        momentum_weight_min: f(v, "MomentumWeightMin").unwrap_or(d.momentum_weight_min),
        momentum_weight_max: f(v, "MomentumWeightMax").unwrap_or(d.momentum_weight_max),
        momentum_min_time: f(v, "MomentumMinTime").unwrap_or(d.momentum_min_time),
        momentum_transition_time: f(v, "MomentumTransitionTime").unwrap_or(d.momentum_transition_time),
        no_input_time: f(v, "NoInputTime").unwrap_or(d.no_input_time),
    })
}

fn dash_from(v: &Value, base: &DashParams) -> DashParams {
    if !v.is_object() {
        return base.clone();
    }
    // Fields a config leaves out default to 0 (instant start / instant stop).
    DashParams {
        distance: f(v, "Distance").unwrap_or(base.distance),
        max_speed: f(v, "MaxSpeed").unwrap_or(base.max_speed),
        accel: f(v, "Acceleration").unwrap_or(0.0),
        decel: f(v, "Deceleration").unwrap_or(0.0),
    }
}

/// Melee from the wrench configs and the clip analysis; anything missing keeps `base`.
fn melee_from(dir: &Path, traversal: &Value, shared: &Value, base: &MeleeParams) -> MeleeParams {
    let mut m = base.clone();
    if let Ok(hop) = jump_from(&traversal["JumpConfigs"]["MeleeSmashJumpConfig"]) {
        m.smash_hop = hop;
    }
    let smash = &shared["MeleeSmash"];
    m.smash_min_height = f(smash, "MeleeSmashMinHeight").unwrap_or(m.smash_min_height);
    m.smash_gravity = f(smash, "MeleeSmashDescendGravity").unwrap_or(m.smash_gravity);

    if let Ok(clips) = read_json(&dir.join("hero_melee_clips.json")) {
        let swings: Vec<SwingParams> = clips["Combo"]
            .as_array()
            .map(|c| c.iter().enumerate().filter_map(|(i, s)| swing_from(s, base.combo.get(i).unwrap_or(&base.combo[0]))).collect())
            .unwrap_or_default();
        if !swings.is_empty() {
            m.combo = swings;
        }
        if let Some(intro) = swing_from(&clips["SmashIntro"], &base.smash_intro) {
            m.smash_intro = intro;
        }
        let land = &clips["SmashLand"];
        let (early, end) = early_transitions(land);
        m.smash_land_time = end.or(f(land, "Duration")).unwrap_or(m.smash_land_time);
        m.smash_land_early_exit = early.unwrap_or(m.smash_land_early_exit).min(m.smash_land_time);
        if let Some(t) = clips["Throw"]["Events"]["WeaponThrown"].get(0).and_then(Value::as_f64) {
            m.throw.release = t as f32;
        }
    }

    let attacks = read_json(&dir.join("wpn_wrench_melee.json")).map(|v| v["MeleeAttackList"].clone()).unwrap_or(Value::Null);
    let attack = |index: &str| attacks.as_array().and_then(|a| a.iter().find(|x| x["Index"] == index)).cloned();
    for (i, swing) in m.combo.iter_mut().enumerate() {
        if let Some(a) = attack(&format!("kMeleeAttack{}", i + 1)) {
            swing.damage = f(&a, "Damage").unwrap_or(swing.damage);
            swing.knockback = f(&a, "OverrideKnockbackDist").unwrap_or(swing.knockback);
        }
    }
    if let Some(a) = attack("kMeleeSmashSlam") {
        m.smash_damage = f(&a, "Damage").unwrap_or(m.smash_damage);
        m.smash_radius = f(&a, "AOE").unwrap_or(m.smash_radius);
    }

    if let Ok(weapon) = read_json(&dir.join("wpn_wrench_weapon.json")) {
        let p = &weapon["Prius"]["Obj"];
        let t = &mut m.throw;
        t.out_speed = f(p, "ThrowOutInitialSpeed").unwrap_or(t.out_speed);
        t.max_distance = f(p, "ThrowMaxDistance").unwrap_or(t.max_distance);
        t.return_max_speed = f(p, "ThrowReturnMaxSpeed").unwrap_or(t.return_max_speed);
        t.return_jerk = f(p, "ThrowReturnJerk").unwrap_or(t.return_jerk);
        t.spin_speed = f(p, "ThrowSpinSpeed").unwrap_or(t.spin_speed);
        t.damage = f(&p["ThrowDamageInfo"], "DamageAmount").unwrap_or(t.damage);
        t.damage_radius = f(&p["ThrowDamageInfo"], "DamageRadius").unwrap_or(t.damage_radius);
    }
    m
}

/// A swing's timing and lunge from `hero_melee_clips.json`; damage and knockback stay `base`'s.
/// Without a second AllowEarlyTransition the swing plays to the clip's end (as in the game).
fn swing_from(v: &Value, base: &SwingParams) -> Option<SwingParams> {
    let duration = f(v, "Duration")?;
    let lunge: Vec<f32> = v["Lunge"].as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default();
    let (early, end) = early_transitions(v);
    let exit = end.unwrap_or(duration).min(duration);
    Some(SwingParams {
        duration,
        hit_time: f(v, "HitTime").unwrap_or(base.hit_time.min(duration)),
        early_exit: early.unwrap_or(exit).min(exit),
        exit,
        lunge,
        lunge_rate: f(v, "Rate").unwrap_or(30.0),
        ..base.clone()
    })
}

/// The first two AllowEarlyTransition events of a clip in `hero_melee_clips.json`.
fn early_transitions(v: &Value) -> (Option<f32>, Option<f32>) {
    let times: Vec<f32> =
        v["Events"]["AllowEarlyTransition"].as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()).unwrap_or_default();
    (times.first().copied(), times.get(1).copied())
}

fn speeds_from(v: &Value, base: &SpeedParams) -> SpeedParams {
    SpeedParams {
        walk_min: f(v, "WalkSpeedMin").unwrap_or(base.walk_min),
        walk_max: f(v, "WalkSpeedMax").unwrap_or(base.walk_max),
        run_min: f(v, "RunSpeedMin").unwrap_or(base.run_min),
        run_max: f(v, "RunSpeedMax").unwrap_or(base.run_max),
        walk_run_threshold: f(v, "WalkRunStickThreshold").unwrap_or(base.walk_run_threshold),
    }
}

pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

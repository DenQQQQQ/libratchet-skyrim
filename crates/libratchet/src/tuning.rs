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
    /// How fast the character turns to face its movement (rad/s).
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
                ..JumpParams::FALLBACK
            },
            double_jump: JumpParams {
                height_min: 1.4,
                height_max: 1.4,
                time_to_peak: 0.3,
                button_max_hold: 0.2,
                no_glide_time: 0.5,
                max_total_jump: Some(2.8),
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
            scale: 1.0,
            source: "generic defaults".into(),
        }
    }
}

impl Tuning {
    /// Builds tuning from the JSON files written by the extractor:
    /// `hero_traversal.json` and `hero_shared.json` in `dir`.
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
            scale: self.scale * s,
            ..self.clone()
        }
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

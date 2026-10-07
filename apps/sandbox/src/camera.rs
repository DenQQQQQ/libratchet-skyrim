//! The follow camera's placement and shakes from Rift Apart's camera configs, extracted from the
//! user's install into `riftapart-configs/` (generic values without them):
//!
//! - `CameraAimContextConfig`: where the camera sits around a pivot above Ratchet's feet,
//!   standing and moving ("traversal"), looking level, down or up, and its field of view. The
//!   default context, and the strafe one (aiming a gun).
//! - `CameraShakingConfig`: three sine curves per axis, held for a duration and then faded out.
//!   The weapons name theirs (the Burst Pistol's recoil, the Shatterbomb's explosion).
//!
//! How the game blends between these isn't decoded, nor the shakes' units. Ours: the offsets are
//! interpolated by pitch and speed, and the shake curves are read as degrees of camera rotation.

use bevy::prelude::*;
use serde_json::Value;
use std::f32::consts::TAU;
use std::path::Path;

/// Pitch (radians, camera above = positive) at which the "looking down" offset applies fully,
/// and the (negative) one for "looking up". Ours.
const PITCH_DOWN: f32 = 1.2;
const PITCH_UP: f32 = -0.6;
/// Speeds (m/s at Ratchet's size) between which the camera moves to its traversal place. Ours.
const TRAVERSAL_SPEEDS: (f32, f32) = (2.5, 6.0);

/// One camera context. Offsets are from the pivot, in metres at Ratchet's size: x to the
/// right, y up, z back.
#[derive(Clone, Debug)]
pub struct CameraContext {
    pub pivot: f32,
    pub level: Vec3,
    pub down: Vec3,
    pub up: Vec3,
    /// Moving fast: pivot height and level offset.
    pub traversal: Option<(f32, Vec3)>,
    /// Horizontal field of view (degrees), standing and moving.
    pub fov: f32,
    pub traversal_fov: f32,
}

impl CameraContext {
    /// Pivot height, offset and horizontal field of view for a camera pitch and how far into
    /// moving fast he is (0..1).
    pub fn place(&self, pitch: f32, moving: f32) -> (f32, Vec3, f32) {
        let key = if pitch >= 0.0 { (self.down, pitch / PITCH_DOWN) } else { (self.up, pitch / PITCH_UP) };
        let mut offset = self.level.lerp(key.0, key.1.clamp(0.0, 1.0));
        let mut pivot = self.pivot;
        if let Some((p, level)) = self.traversal {
            offset += (level - self.level) * moving;
            pivot += (p - self.pivot) * moving;
        }
        (pivot, offset, self.fov + (self.traversal_fov - self.fov) * moving)
    }

    /// How far into its traversal place the camera should be at this ground speed.
    pub fn moving(speed: f32) -> f32 {
        ((speed - TRAVERSAL_SPEEDS.0) / (TRAVERSAL_SPEEDS.1 - TRAVERSAL_SPEEDS.0)).clamp(0.0, 1.0)
    }

    fn from_json(v: &Value, base: &CameraContext) -> CameraContext {
        // The game's offsets: x to the left, z forward.
        let offset = |key: &str| {
            let o = &v[key];
            o.is_object().then(|| Vec3::new(-num(&o["X"]).unwrap_or(0.0), num(&o["Y"]).unwrap_or(0.0), -num(&o["Z"]).unwrap_or(0.0)))
        };
        let level = offset("StandOffsetLevel").unwrap_or(base.level);
        let fov = num(&v["FieldOfViewStand"]).unwrap_or(base.fov);
        CameraContext {
            pivot: num(&v["StandPivotHeight"]).unwrap_or(base.pivot),
            level,
            down: offset("StandOffsetDown").unwrap_or(level),
            up: offset("StandOffsetUp").unwrap_or(level),
            traversal: offset("TraversalOffsetLevel").map(|l| (num(&v["TraversalPivotHeight"]).unwrap_or(base.pivot), l)),
            fov,
            traversal_fov: num(&v["FieldOfViewTraversal"]).unwrap_or(fov),
        }
    }
}

/// A camera shake: per axis, three sine curves (magnitude, frequency in Hz, phase in turns).
#[derive(Clone, Debug, Default)]
pub struct Shake {
    pub curves: [[(f32, f32, f32); 3]; 3],
    /// Full strength this long, then fading out over `fadeout`.
    pub duration: f32,
    pub fadeout: f32,
}

impl Shake {
    /// The shake `t` seconds in (x, y, z; read as degrees of yaw, pitch and roll), or `None`
    /// once it's over.
    pub fn sample(&self, t: f32) -> Option<Vec3> {
        if t > self.duration + self.fadeout {
            return None;
        }
        let envelope = if t <= self.duration { 1.0 } else { 1.0 - (t - self.duration) / self.fadeout.max(1e-4) };
        let axis = |curves: &[(f32, f32, f32); 3]| curves.iter().map(|(m, f, p)| m * (TAU * (f * t + p)).sin()).sum::<f32>();
        Some(Vec3::new(axis(&self.curves[0]), axis(&self.curves[1]), axis(&self.curves[2])) * envelope)
    }

    /// Reads `<dir>/<name>.json` (a `CameraShakingConfig`).
    pub fn load(dir: &Path, name: &str) -> Option<Shake> {
        let text = std::fs::read_to_string(dir.join(format!("{name}.json"))).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        if v["_type"] != "CameraShakingConfig" {
            return None;
        }
        let d = &v["Data"];
        let curve = |c: &Value| (num(&c["Magnitude"]).unwrap_or(0.0), num(&c["Frequency"]).unwrap_or(0.0), num(&c["Offset"]).unwrap_or(0.0));
        let axis = |a: &Value| [curve(&a["CurveOne"]), curve(&a["CurveTwo"]), curve(&a["CurveThree"])];
        Some(Shake {
            curves: [axis(&d["CurveDataX"]), axis(&d["CurveDataY"]), axis(&d["CurveDataZ"])],
            duration: num(&d["Duration"]).unwrap_or(0.0),
            fadeout: num(&d["FadeoutDuration"]).unwrap_or(0.0),
        })
    }
}

/// The two camera contexts.
#[derive(Resource, Clone, Debug)]
pub struct CameraConfigs {
    pub stand: CameraContext,
    pub aim: CameraContext,
    /// Where they came from, for the log.
    pub source: &'static str,
}

impl CameraConfigs {
    /// Generic values (written for this project).
    pub fn generic() -> CameraConfigs {
        CameraConfigs {
            stand: CameraContext {
                pivot: 1.4,
                level: Vec3::new(0.0, 0.0, 5.0),
                down: Vec3::new(0.0, 0.4, 5.0),
                up: Vec3::new(0.0, 0.5, 3.0),
                traversal: Some((1.5, Vec3::new(0.0, 0.2, 6.0))),
                fov: 75.0,
                traversal_fov: 80.0,
            },
            aim: CameraContext {
                pivot: 1.5,
                level: Vec3::new(0.6, 0.0, 4.0),
                down: Vec3::new(0.6, 0.0, 3.0),
                up: Vec3::new(0.8, 0.0, 1.5),
                traversal: None,
                fov: 70.0,
                traversal_fov: 70.0,
            },
            source: "generic",
        }
    }

    pub fn load(dir: &Path) -> CameraConfigs {
        let read = |name: &str| -> Option<Value> {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.json"))).ok()?).ok()?;
            (v["_type"] == "CameraAimContextConfig").then_some(v)
        };
        let generic = CameraConfigs::generic();
        match (read("camera_Default_AimContext"), read("camera_Strafe_AimContext")) {
            (Some(stand), Some(aim)) => CameraConfigs {
                stand: CameraContext::from_json(&stand, &generic.stand),
                aim: CameraContext::from_json(&aim, &generic.aim),
                source: "Rift Apart camera configs",
            },
            _ => generic,
        }
    }
}

/// Says where the camera's numbers came from.
pub fn log_source(configs: Res<CameraConfigs>) {
    info!("camera: {}", configs.source);
}

fn num(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_holds_then_fades_then_ends() {
        let shake = Shake { curves: [[(1.0, 1.0, 0.25), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0)]; 3], duration: 0.5, fadeout: 0.5 };
        // sin(2π·0.25) = 1 at t = 0: full strength.
        assert!((shake.sample(0.0).unwrap().x - 1.0).abs() < 1e-5);
        // Halfway through the fade, half strength (the curve is back at its peak at t = 1).
        let t = 0.75;
        let full = (TAU * (t + 0.25)).sin();
        assert!((shake.sample(t).unwrap().x - full * 0.5).abs() < 1e-4);
        assert!(shake.sample(1.01).is_none());
    }

    #[test]
    fn camera_moves_toward_the_down_offset_as_it_pitches_down() {
        let c = CameraConfigs::generic().stand;
        let (_, level, _) = c.place(0.0, 0.0);
        let (_, down, _) = c.place(PITCH_DOWN, 0.0);
        assert_eq!(level, c.level);
        assert_eq!(down, c.down);
        let (_, run, fov) = c.place(0.0, 1.0);
        assert_eq!(run, c.traversal.unwrap().1);
        assert_eq!(fov, c.traversal_fov);
    }
}

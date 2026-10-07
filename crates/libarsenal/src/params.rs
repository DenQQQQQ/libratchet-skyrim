//! Weapon parameters, shaped like Rift Apart's `WeaponConfig` (trigger, ammo, level data, shot
//! motion, explosion), with generic defaults and a loader for the user's extracted JSON.
//!
//! Units: metres, seconds, degrees for angles.

use glam::Vec3;
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Fires for as long as the trigger is held (`kAutoShot`).
    Auto,
    /// One shot per press (`kSingleShot`, `kCookedSingleShot`).
    Single,
}

/// How a shot flies.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectileParams {
    pub speed: f32,
    /// Downward acceleration (m/s²); 0 flies straight.
    pub gravity: f32,
    /// Distance after which a straight shot disappears.
    pub range: f32,
    /// Seconds before it disappears (or explodes) anyway.
    pub lifetime: f32,
    /// Collision radius.
    pub radius: f32,
    /// Tumbling speed for the visual (degrees per second).
    pub spin: f32,
}

/// The spread cone: it widens as you keep firing and narrows when you stop.
#[derive(Clone, Debug, PartialEq)]
pub struct SpreadParams {
    /// Cone half-angle of the first shots (degrees).
    pub initial: f32,
    /// How much each shot widens it, after `free_shots` shots in a row.
    pub per_shot: f32,
    pub max: f32,
    /// How fast it narrows when not firing (degrees per second).
    pub recover: f32,
    pub free_shots: u32,
}

impl SpreadParams {
    pub const NONE: SpreadParams = SpreadParams { initial: 0.0, per_shot: 0.0, max: 0.0, recover: 0.0, free_shots: 0 };
}

/// Damage and knockback around an explosion.
#[derive(Clone, Debug, PartialEq)]
pub struct ExplosionParams {
    /// Full damage inside this radius...
    pub inner_radius: f32,
    /// ...falling off linearly to `outer_damage` at this one; nothing beyond.
    pub outer_radius: f32,
    pub inner_damage: f32,
    pub outer_damage: f32,
    /// (up to this distance, knockback in metres), nearest first.
    pub knockback: Vec<(f32, f32)>,
}

impl ExplosionParams {
    /// Damage at `distance` from the centre.
    pub fn damage_at(&self, distance: f32) -> f32 {
        if distance <= self.inner_radius {
            self.inner_damage
        } else if distance <= self.outer_radius {
            let t = (distance - self.inner_radius) / (self.outer_radius - self.inner_radius).max(1e-6);
            self.inner_damage + (self.outer_damage - self.inner_damage) * t
        } else {
            0.0
        }
    }

    /// How far a standard target at `distance` is knocked back.
    pub fn knockback_at(&self, distance: f32) -> f32 {
        self.knockback.iter().find(|(range, _)| distance <= *range).map_or(0.0, |(_, push)| *push)
    }

    fn scaled(&self, s: f32) -> ExplosionParams {
        ExplosionParams {
            inner_radius: self.inner_radius * s,
            outer_radius: self.outer_radius * s,
            knockback: self.knockback.iter().map(|(r, k)| (r * s, k * s)).collect(),
            ..self.clone()
        }
    }
}

/// Spent shells the gun throws out with each shot (`ShellCasingData`); only for looks.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellParams {
    /// Ejection speed range (m/s).
    pub speed: (f32, f32),
    /// Largest tumbling speed about each axis of the ejector. The config's unit isn't known;
    /// read as radians per second (as degrees the shells would barely turn).
    pub spin: Vec3,
}

/// A camera shake a weapon asks for: the game config that describes it (by name), and for
/// explosions, full strength within `inner` metres, fading to nothing at `outer`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShakeRef {
    pub config: String,
    pub inner: f32,
    pub outer: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeaponParams {
    pub name: String,
    pub trigger: Trigger,
    /// Shots per second while aiming (or for every shot, if `rapid_rate` is the same).
    pub rate: f32,
    /// Shots per second when firing from the hip.
    pub rapid_rate: f32,
    pub ammo: u32,
    pub ammo_cost: u32,
    /// Damage of a direct hit (for explosives: the explosion's own numbers apply).
    pub damage: f32,
    /// How far a direct hit pushes a standard target (metres).
    pub knockback: f32,
    pub projectile: ProjectileParams,
    /// Spread from the hip, and while aiming.
    pub spread: SpreadParams,
    pub aimed_spread: SpreadParams,
    /// Lobbed: the launch velocity is solved to land on the aimed point (grenades).
    pub lob: bool,
    /// Furthest a lobbed shot is aimed.
    pub lob_range: f32,
    pub explosion: Option<ExplosionParams>,
    pub shell: Option<ShellParams>,
    /// The camera shake of each shot (recoil), and of its explosion.
    pub recoil_shake: Option<ShakeRef>,
    pub blast_shake: Option<ShakeRef>,
    /// Where these values came from, for display.
    pub source: String,
}

impl WeaponParams {
    /// A generic rapid-fire pistol (values written for this project).
    pub fn pistol() -> WeaponParams {
        WeaponParams {
            name: "Pistol".into(),
            trigger: Trigger::Auto,
            rate: 4.0,
            rapid_rate: 7.0,
            ammo: 120,
            ammo_cost: 1,
            damage: 10.0,
            knockback: 0.3,
            projectile: ProjectileParams { speed: 70.0, gravity: 0.0, range: 50.0, lifetime: 2.0, radius: 0.2, spin: 0.0 },
            spread: SpreadParams { initial: 0.2, per_shot: 0.1, max: 3.0, recover: 20.0, free_shots: 3 },
            aimed_spread: SpreadParams { initial: 0.1, per_shot: 0.02, max: 0.6, recover: 5.0, free_shots: 3 },
            lob: false,
            lob_range: 0.0,
            explosion: None,
            shell: None,
            recoil_shake: None,
            blast_shake: None,
            source: "generic defaults".into(),
        }
    }

    /// A generic impact grenade (values written for this project).
    pub fn grenade() -> WeaponParams {
        WeaponParams {
            name: "Grenade".into(),
            trigger: Trigger::Single,
            rate: 1.0,
            rapid_rate: 1.0,
            ammo: 8,
            ammo_cost: 1,
            damage: 5.0,
            knockback: 0.5,
            projectile: ProjectileParams { speed: 25.0, gravity: 25.0, range: 60.0, lifetime: 6.0, radius: 0.25, spin: 400.0 },
            spread: SpreadParams::NONE,
            aimed_spread: SpreadParams::NONE,
            lob: true,
            lob_range: 25.0,
            explosion: Some(ExplosionParams {
                inner_radius: 1.0,
                outer_radius: 5.0,
                inner_damage: 150.0,
                outer_damage: 60.0,
                knockback: vec![(2.5, 3.5), (5.0, 1.5), (15.0, 0.4)],
            }),
            shell: None,
            recoil_shake: None,
            blast_shake: None,
            source: "generic defaults".into(),
        }
    }

    /// Reads a Rift Apart `WeaponConfig` extracted as JSON (`wpn_<name>_weapon.json`). Level-1
    /// upgrade values (damage, rate of fire, ammo, range) win over the base fields, as in the
    /// game; anything missing keeps `base`.
    pub fn from_riftapart_config(path: &Path, base: &WeaponParams) -> Result<WeaponParams, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let w: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if w["_type"] != "WeaponConfig" {
            return Err(format!("{}: not a WeaponConfig", path.display()));
        }
        let level = level_mods(&w);
        let mod_value = |name: &str| level.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
        let trigger = &w["TriggerList"]["Trigger"]["Obj"];
        let shot = &w["ShotList"]["Shot"]["Obj"]["ShotPrius"]["Obj"];
        let motion = &shot["InitialMotion"]["Obj"];
        let prius = &w["Prius"]["Obj"];

        let mut p = base.clone();
        p.name = w["WeaponName"].as_str().unwrap_or(&base.name).to_string();
        p.trigger = match trigger["TriggerType"].as_str() {
            Some("kAutoShot") => Trigger::Auto,
            Some(_) => Trigger::Single,
            None => base.trigger,
        };
        p.rate = mod_value("kRateOfFire").or_else(|| f(trigger, "RoundsPerSecond")).unwrap_or(base.rate);
        // Guess: the Burst Pistol's kMisc3 (6) is its full-trigger rapid-fire rate.
        p.rapid_rate = if p.trigger == Trigger::Auto { mod_value("kMisc3").unwrap_or(p.rate).max(p.rate) } else { p.rate };
        p.ammo = mod_value("kAmmo").or_else(|| f(&w["AmmoList"]["Ammo"]["Obj"], "MaxAmmo")).map_or(base.ammo, |a| a as u32);
        p.ammo_cost = f(&w, "AmmoCost").map_or(base.ammo_cost, |a| a.max(1.0) as u32);
        p.damage = mod_value("kDamage").unwrap_or(base.damage);
        if motion.is_object() {
            let pp = &mut p.projectile;
            pp.speed = f(motion, "FlightSpeed").unwrap_or(pp.speed);
            pp.gravity = f(motion, "Gravity").unwrap_or(pp.gravity);
            pp.range = f(motion, "Range").unwrap_or(pp.range);
            pp.radius = f(motion, "CollRadius").unwrap_or(pp.radius);
            pp.spin = f(&motion["Orientation"]["Obj"], "RotationalSpeed").unwrap_or(0.0);
            p.lob = motion["VelocityMode"]["Type"] == "VelocityFromTarget";
        }
        p.projectile.lifetime = f(shot, "LifeTime").unwrap_or(p.projectile.range / p.projectile.speed.max(1.0));
        if p.lob {
            p.lob_range = mod_value("kRange").unwrap_or(base.lob_range.max(p.projectile.speed * p.projectile.speed / p.projectile.gravity.max(1.0)));
        }
        if let Some(s) = spread_from(&prius["InaccuracyData"]) {
            p.spread = s;
        }
        if let Some(s) = spread_from(&prius["SlowInaccuracyData"]) {
            p.aimed_spread = s;
        }
        let recoil = &prius["Recoil"]["CameraShake"]["CameraShake"]["Obj"]["CameraShakeData"];
        if let Some(name) = recoil.as_str().and_then(config_name) {
            p.recoil_shake = Some(ShakeRef { config: name, inner: f32::INFINITY, outer: f32::INFINITY });
        }
        let blast = &shot["ShakeData"];
        if let Some(name) = blast["ShakeConfig"].as_str().and_then(config_name) {
            let inner = f(blast, "InnerRadius").unwrap_or(f32::INFINITY);
            p.blast_shake = Some(ShakeRef { config: name, inner, outer: f(blast, "OuterRadius").unwrap_or(inner).max(inner) });
        }
        let shell = &prius["ShellCasingData"];
        if let (Some(lo), Some(hi)) = (f(shell, "EjectSpeedMin"), f(shell, "EjectSpeedMax")) {
            let spin = &shell["EjectSpinMax"];
            let axis = |k: &str| f(spin, k).unwrap_or(0.0);
            p.shell = Some(ShellParams { speed: (lo, hi), spin: Vec3::new(axis("X"), axis("Y"), axis("Z")) });
        }
        let ex = &shot["ExplosionData"];
        if ex.is_object() {
            let b = base.explosion.clone().unwrap_or_else(|| WeaponParams::grenade().explosion.unwrap());
            let outer = mod_value("kAOEOuter").or_else(|| f(ex, "DamageOuterRadius")).unwrap_or(b.outer_radius);
            let tiers: Vec<(f32, f32)> = ex["KnockbackData"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|k| Some((f(k, "RangeMax")?, knockback_level(k["KnockbackLevel"].as_str()?)?)))
                        .collect()
                })
                .unwrap_or_default();
            p.explosion = Some(ExplosionParams {
                inner_radius: f(ex, "DamageInnerRadius").unwrap_or(b.inner_radius),
                outer_radius: outer,
                inner_damage: p.damage,
                outer_damage: mod_value("kAltDamageOuter").or(mod_value("kAltDamage")).unwrap_or(p.damage * 0.5),
                knockback: if tiers.is_empty() { b.knockback } else { tiers },
            });
            // The level damage is the explosion's; a direct hit only does the shot's own bit.
            p.damage = f(shot, "ImpactDamage").unwrap_or(0.0);
        }
        p.source = format!("Rift Apart config ({})", path.file_name().and_then(|n| n.to_str()).unwrap_or("?"));
        Ok(p)
    }

    /// Distances and speeds times `s` (damage, rates and timing stay), like libratchet's scaling.
    pub fn scaled(&self, s: f32) -> WeaponParams {
        let mut p = self.clone();
        p.knockback *= s;
        p.lob_range *= s;
        let pp = &mut p.projectile;
        pp.speed *= s;
        pp.gravity *= s;
        pp.range *= s;
        pp.radius *= s;
        p.explosion = self.explosion.as_ref().map(|e| e.scaled(s));
        if let Some(shell) = p.shell.as_mut() {
            shell.speed = (shell.speed.0 * s, shell.speed.1 * s);
        }
        for shake in [&mut p.recoil_shake, &mut p.blast_shake].into_iter().flatten() {
            shake.inner *= s;
            shake.outer *= s;
        }
        p
    }
}

/// Knockback distance for the game's knockback levels (ours: the levels' real distances aren't
/// known).
fn knockback_level(level: &str) -> Option<f32> {
    Some(match level {
        "kOne" => 0.4,
        "kTwo" => 1.5,
        "kThree" => 2.5,
        "kFour" => 3.5,
        _ => return None,
    })
}

/// The first level's upgrades as (ModType, value).
fn level_mods(w: &Value) -> Vec<(String, f32)> {
    w["LevelList"]
        .as_array()
        .and_then(|l| l.first())
        .and_then(|l| l["UpgradeList"].as_array())
        .map(|ups| ups.iter().filter_map(|u| Some((u["ModType"].as_str()?.to_string(), f(u, "ModValue").unwrap_or(0.0)))).collect())
        .unwrap_or_default()
}

fn spread_from(v: &Value) -> Option<SpreadParams> {
    if !v.is_object() {
        return None;
    }
    Some(SpreadParams {
        initial: f(v, "InitialAngle").unwrap_or(0.0),
        per_shot: f(v, "IncreaseAnglePerShot").unwrap_or(0.0),
        max: f(v, "AngleMax").unwrap_or(0.0),
        recover: f(v, "DecreaseAnglePerSec").unwrap_or(10.0),
        free_shots: f(v, "MinShotsToIncrease").map_or(0, |n| n as u32),
    })
}

/// "configs/camera/camera_x.config" -> "camera_x" (the extracted JSON's file name).
fn config_name(path: &str) -> Option<String> {
    let base = path.rsplit(['/', '\\']).next()?;
    let name = base.strip_suffix(".config").unwrap_or(base);
    (!name.is_empty()).then(|| name.to_string())
}

fn f(v: &Value, key: &str) -> Option<f32> {
    v.get(key).and_then(Value::as_f64).map(|x| x as f32)
}

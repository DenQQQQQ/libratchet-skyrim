//! Guns, through libarsenal: the Burst Pistol and the Shatterbomb. Keys 1/2/3 pick the wrench,
//! the pistol or the grenade. With a gun out, left mouse fires and right mouse aims (strafing,
//! over-the-shoulder camera). Shots go where the crosshair points: the camera ray's first hit on
//! the level or a dummy.
//!
//! Weapon numbers come from `riftapart-configs/wpn_burstpistol_weapon.json` and
//! `wpn_protogrenade_weapon.json` (extracted from the user's own install), else generic defaults.

use crate::camera::Shake;
use crate::combat::{Dummy, Hits, dummy_contains, hit};
use crate::level::{Level, ground_distance};
use crate::vfx::{FxRng, VfxArt};
use crate::{CameraRig, MouseMelee, Player, Settings, simulate};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use libarsenal::{Aim, Projectile, Weapon, WeaponParams};
use libratchet::MoveState;
use std::path::Path;
use std::time::Duration;

pub const PISTOL_FILE: &str = "burstpistol.glb";
pub const GRENADE_FILE: &str = "shatterbomb.glb";
/// How far the crosshair ray looks for something to aim at.
const AIM_RANGE: f32 = 80.0;
/// Without the converter's ratchet.json, a grenade leaves the hand this long into the throw
/// (ours; with it, the throw clips' AnimFireWeapon events).
const GRENADE_RELEASE: f32 = 0.1;
/// Empty weapons refill after this long (a sandbox convenience).
const REFILL_AFTER: f32 = 1.5;
/// Ratchet strafes (facing the aim) while a gun fires and for this long after the newest shot,
/// then goes back to running; there's no lingering gun-up stance (RiftApart.exe).
const STRAFE_AFTER_SHOT: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Wrench,
    BurstPistol,
    Shatterbomb,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Slot::Wrench => "OmniWrench",
            Slot::BurstPistol => "Burst Pistol",
            Slot::Shatterbomb => "Shatterbomb",
        }
    }
    pub fn is_gun(self) -> bool {
        self != Slot::Wrench
    }
}

/// The demo script's weapon controls, overriding the mouse when set.
#[derive(Resource, Default)]
pub struct ScriptedArsenal {
    pub active: bool,
    pub trigger: bool,
    pub aim: bool,
    /// Aim here instead of at the crosshair.
    pub target: Option<Vec3>,
}

#[derive(Resource)]
pub struct Arsenal {
    pub slot: Slot,
    pub pistol: Weapon,
    pub grenade: Weapon,
    /// Aiming (right mouse, with any weapon) and trigger (left mouse, with a gun out) this frame.
    pub aiming: bool,
    pub trigger: bool,
    /// Counters the animation watches to start the recoil layer / grenade throw.
    pub shots: u32,
    pub throws: u32,
    /// Seconds left of strafing after the newest shot.
    pub stance: f32,
    /// When a thrown grenade leaves the hand: standing still, and on the move or in the air.
    grenade_release: (f32, f32),
    pending: Option<f32>,
    refill: f32,
    pub note: String,
    // The pistol model in the hand.
    hand: Option<Entity>,
    gun: Option<Entity>,
    gun_player: Option<Entity>,
    gun_fire: Option<AnimationNodeIndex>,
    muzzle: Option<Entity>,
    /// The gun's shell ejector (its igLoc_shell_eject locator).
    ejector: Option<Entity>,
    shots_seen: u32,
}

/// The game's camera shakes the weapons name (recoil per shot; explosion, with the radii it
/// fades over).
#[derive(Resource, Default)]
struct WeaponShakes {
    recoil: Option<Shake>,
    blast: Option<(Shake, f32, f32)>,
}

#[derive(Resource)]
struct WeaponModels {
    pistol: Option<Handle<Gltf>>,
    grenade: Option<Handle<WorldAsset>>,
}

#[derive(Component)]
struct Bolt {
    proj: Projectile,
    damage: f32,
    knockback: f32,
    /// It hit something this step: shown at the hit for one step, then removed.
    spent: bool,
}

#[derive(Component)]
struct Grenade {
    proj: Projectile,
}

#[derive(Component)]
struct Blast {
    age: f32,
    radius: f32,
    life: f32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ScriptedArsenal>()
        .add_systems(Startup, setup.after(crate::setup))
        .add_systems(FixedUpdate, (fire, fly_bolts, fly_grenades).chain().after(simulate))
        .add_systems(Update, (controls, gun_clips, gun_visibility, blasts).chain());
}

fn load_weapon(settings: &Settings, file: &str, base: WeaponParams) -> (WeaponParams, String) {
    let path = settings.configs_dir.join(file);
    match WeaponParams::from_riftapart_config(&path, &base) {
        Ok(p) => (p, String::new()),
        Err(_) if !Path::new(&path).exists() => (base, format!("{file} not found, generic values")),
        Err(e) => (base, e),
    }
}

/// The throw clips' AnimFireWeapon events (the whole-body throw, the upper-body one), from the
/// converter's ratchet.json.
fn grenade_release(settings: &Settings) -> (f32, f32) {
    let manifest = settings.model_dir.as_ref().map(|d| d.join("ratchet.json"));
    let event = |clip: &str| manifest.as_ref().and_then(|m| crate::ratchet_model::clip_event(m, clip, "AnimFireWeapon"));
    (event("grenade_throw").unwrap_or(GRENADE_RELEASE), event("grenade_throw_arm").unwrap_or(GRENADE_RELEASE))
}

fn setup(mut commands: Commands, settings: Res<Settings>, assets: Res<AssetServer>) {
    let (pistol, n1) = load_weapon(&settings, "wpn_burstpistol_weapon.json", WeaponParams::pistol());
    let (grenade, n2) = load_weapon(&settings, "wpn_protogrenade_weapon.json", WeaponParams::grenade());
    let note = [n1, n2].into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("; ");
    let s = settings.scale;
    let shake = |r: &Option<libarsenal::ShakeRef>| {
        let r = r.as_ref()?;
        Shake::load(&settings.configs_dir, &r.config).map(|sh| (sh, r.inner * s, r.outer * s))
    };
    commands.insert_resource(WeaponShakes { recoil: shake(&pistol.recoil_shake).map(|(sh, _, _)| sh), blast: shake(&grenade.blast_shake) });
    commands.insert_resource(Arsenal {
        slot: Slot::Wrench,
        pistol: Weapon::new(pistol.scaled(s)),
        grenade: Weapon::new(grenade.scaled(s)),
        aiming: false,
        trigger: false,
        shots: 0,
        throws: 0,
        stance: 0.0,
        grenade_release: grenade_release(&settings),
        pending: None,
        refill: 0.0,
        note,
        hand: None,
        gun: None,
        gun_player: None,
        gun_fire: None,
        muzzle: None,
        ejector: None,
        shots_seen: 0,
    });
    let has = |f: &str| settings.model_dir.as_ref().is_some_and(|d| d.join(f).exists());
    commands.insert_resource(WeaponModels {
        pistol: has(PISTOL_FILE).then(|| assets.load(PISTOL_FILE)),
        grenade: has(GRENADE_FILE).then(|| assets.load(GltfAssetLabel::Scene(0).from_asset(GRENADE_FILE))),
    });
}

/// Puts the Burst Pistol in Ratchet's right hand (the gun-attach locator's joint), under a
/// holder we own for visibility. Called once his model is ready.
pub fn spawn_gun(commands: &mut Commands, assets: &AssetServer, hand: Entity) {
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(PISTOL_FILE));
    let holder = commands.spawn((Name::new("Burst Pistol"), Transform::default(), Visibility::Hidden)).id();
    commands.entity(hand).add_child(holder);
    let gun = commands.spawn((WorldAssetRoot(scene), Transform::default())).observe(on_gun_ready).id();
    commands.entity(holder).add_child(gun);
    commands.queue(move |world: &mut World| {
        if let Some(mut a) = world.get_resource_mut::<Arsenal>() {
            a.hand = Some(hand);
            a.gun = Some(holder);
        }
    });
}

fn on_gun_ready(
    ev: On<WorldInstanceReady>,
    mut arsenal: ResMut<Arsenal>,
    children: Query<&Children>,
    names: Query<&Name>,
    players: Query<(), With<AnimationPlayer>>,
    mut commands: Commands,
) {
    commands.entity(ev.entity).insert(Visibility::Inherited);
    for e in children.iter_descendants(ev.entity) {
        if players.contains(e) {
            arsenal.gun_player = Some(e);
        }
        // The game's locators (exported as nodes); older exports only have the muzzle's joint.
        match names.get(e).map(|n| n.as_str()) {
            Ok("igLoc_bulletEmit") => arsenal.muzzle = Some(e),
            Ok("CTR_chamber") if arsenal.muzzle.is_none() => arsenal.muzzle = Some(e),
            Ok("igLoc_shell_eject") => arsenal.ejector = Some(e),
            _ => {}
        }
    }
}

/// Number keys pick the weapon; right mouse aims (with any weapon, as in the game, where aim + melee
/// throws the wrench); with a gun out, left mouse fires.
#[allow(clippy::too_many_arguments)]
fn controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_melee: Res<MouseMelee>,
    scripted: Res<ScriptedArsenal>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut arsenal: ResMut<Arsenal>,
) {
    for (key, slot) in [(KeyCode::Digit1, Slot::Wrench), (KeyCode::Digit2, Slot::BurstPistol), (KeyCode::Digit3, Slot::Shatterbomb)] {
        if keys.just_pressed(key) {
            arsenal.slot = slot;
        }
    }
    let gun = arsenal.slot.is_gun();
    if scripted.active {
        arsenal.aiming = scripted.aim;
        arsenal.trigger = gun && scripted.trigger;
    } else {
        let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
        arsenal.aiming = grabbed && mouse.pressed(MouseButton::Right);
        arsenal.trigger = gun && grabbed && mouse_melee.armed() && mouse.pressed(MouseButton::Left);
    }
    arsenal.stance = if gun { (arsenal.stance - time.delta_secs()).max(0.0) } else { 0.0 };
}

/// Camera ray: where the crosshair (screen centre) points.
fn crosshair_target(cam: &GlobalTransform, level: &Level, dummies: &Query<&mut Dummy>, range: f32) -> Vec3 {
    let from = cam.translation();
    let dir = cam.forward().as_vec3();
    let mut best = range;
    for b in level.blocks.iter() {
        if let Some(t) = ray_box(from, dir, b.min, b.max)
            && t < best
        {
            best = t;
        }
    }
    // Dummies: march along the ray (they're small and few).
    let step = 0.1;
    let mut t = 0.0;
    while t < best {
        let p = from + dir * t;
        if dummies.iter().any(|d| dummy_contains(d, p, 0.0)) {
            best = t;
            break;
        }
        t += step;
    }
    from + dir * best
}

/// Distance along the ray to an axis-aligned box, if it hits (slab test).
fn ray_box(from: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    ray_box_normal(from, dir, min, max).map(|(t, _)| t)
}

/// Like `ray_box`, with the normal of the face it enters through.
fn ray_box_normal(from: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    let inv = dir.recip();
    let t1 = (min - from) * inv;
    let t2 = (max - from) * inv;
    let lo = t1.min(t2);
    let near = lo.max_element();
    let far = t1.max(t2).min_element();
    if far < near.max(0.0) {
        return None;
    }
    let axis = if lo.x >= lo.y && lo.x >= lo.z { 0 } else if lo.y >= lo.z { 1 } else { 2 };
    let mut n = Vec3::ZERO;
    n[axis] = -dir[axis].signum();
    Some((near.max(0.0), n))
}

/// Where along a→b (0..1) the segment enters a box, and that face's normal.
fn segment_hits_box(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    let d = b - a;
    let len = d.length();
    if len < 1e-6 {
        return (a.cmpge(min).all() && a.cmple(max).all()).then_some((0.0, Vec3::Y));
    }
    ray_box_normal(a, d / len, min, max).filter(|(t, _)| *t <= len).map(|(t, n)| (t / len, n))
}

fn muzzle_position(arsenal: &Arsenal, globals: &Query<&GlobalTransform>, p: &Player, scale: f32) -> Vec3 {
    let hand = arsenal.hand.and_then(|h| globals.get(h).ok()).map(|g| g.translation());
    match (arsenal.slot, arsenal.muzzle.and_then(|m| globals.get(m).ok())) {
        (Slot::BurstPistol, Some(m)) => m.translation(),
        _ => hand.unwrap_or(p.feet + Vec3::Y * 1.0 * scale + p.sim.facing_dir() * 0.3 * scale),
    }
}

/// Runs the selected weapon and spawns what it fires.
#[allow(clippy::too_many_arguments)]
fn fire(
    time: Res<Time>,
    settings: Res<Settings>,
    level: Res<Level>,
    models: Res<WeaponModels>,
    mut arsenal: ResMut<Arsenal>,
    players: Query<&Player>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    globals: Query<&GlobalTransform>,
    dummies: Query<&mut Dummy>,
    scripted: Res<ScriptedArsenal>,
    (art, mut rng): (Res<VfxArt>, ResMut<FxRng>),
    (mut rig, shakes): (ResMut<CameraRig>, Res<WeaponShakes>),
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let (Ok(p), Ok(cam)) = (players.single(), cams.single()) else { return };
    let busy = matches!(p.sim.state(), MoveState::Melee | MoveState::Smash | MoveState::SmashLand);
    let target = match scripted.target {
        Some(t) if scripted.active => t,
        _ => crosshair_target(cam, &level, &dummies, AIM_RANGE * settings.scale),
    };
    let muzzle = muzzle_position(&arsenal, &globals, p, settings.scale);
    let aim = Aim { muzzle, target, aimed: arsenal.aiming };
    let trigger = arsenal.trigger && !busy;

    // A thrown grenade leaves the hand a moment into the throw.
    if let Some(t) = arsenal.pending.as_mut() {
        *t -= dt;
        if *t <= 0.0 {
            arsenal.pending = None;
            arsenal.stance = STRAFE_AFTER_SHOT;
            if let Some(shot) = arsenal.grenade.step(0.0, true, &aim).into_iter().next().or_else(|| last_grenade_shot(&arsenal, &aim)) {
                let proj = Projectile::new(&shot, &arsenal.grenade.params().projectile);
                let light = art.available.then(|| art.grenade_light(settings.scale));
                spawn_grenade(&mut commands, &models, &mut meshes, &mut materials, proj, light, settings.scale);
            }
        }
    }

    match arsenal.slot {
        Slot::BurstPistol => {
            let shots = arsenal.pistol.step(dt, trigger, &aim);
            let w = arsenal.pistol.params().clone();
            let s = settings.scale;
            for shot in shots {
                arsenal.shots += 1;
                arsenal.stance = STRAFE_AFTER_SHOT;
                if let Some(recoil) = &shakes.recoil {
                    rig.add_shake(recoil, 1.0);
                }
                let proj = Projectile::new(&shot, &w.projectile);
                let dir = shot.velocity.normalize();
                if art.available {
                    // The game's muzzle flash, bolt, and a spent shell out of the ejector.
                    art.muzzle_flash(&mut commands, &mut materials, &mut rng, shot.origin, dir, s);
                    let bolt = commands
                        .spawn((
                            Bolt { proj, damage: w.damage, knockback: w.knockback, spent: false },
                            Transform::from_translation(shot.origin).with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                            Visibility::default(),
                        ))
                        .with_children(|c| art.bolt(c, &mut materials, s))
                        .id();
                    art.streak(&mut commands, &mut materials, bolt, shot.origin, s);
                    if let (Some(shell), Some(eject)) = (&w.shell, arsenal.ejector.and_then(|e| globals.get(e).ok())) {
                        let (_, rot, at) = eject.to_scale_rotation_translation();
                        let floor = ground_distance(&level, at, 0.05).map(|d| at.y - d);
                        art.shell(&mut commands, &mut rng, at, rot, shell, floor, s);
                    }
                    continue;
                }
                // Muzzle flash.
                commands.spawn((
                    Blast { age: 0.0, radius: 0.12 * settings.scale, life: 0.06 },
                    Mesh3d(meshes.add(Sphere::new(1.0))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(1.0, 0.85, 0.4, 0.9),
                        emissive: LinearRgba::rgb(12.0, 8.0, 2.0),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        ..default()
                    })),
                    Transform::from_translation(shot.origin).with_scale(Vec3::splat(0.05)),
                ));
                commands.spawn((
                    Bolt { proj, damage: w.damage, knockback: w.knockback, spent: false },
                    Mesh3d(meshes.add(Capsule3d::new(0.04 * settings.scale, 0.9 * settings.scale))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.8, 0.3),
                        emissive: LinearRgba::rgb(8.0, 4.5, 1.0),
                        unlit: true,
                        ..default()
                    })),
                    Transform::from_translation(shot.origin).with_rotation(Quat::from_rotation_arc(Vec3::Y, dir)),
                ));
            }
        }
        Slot::Shatterbomb => {
            // The weapon decides (rate, ammo); the grenade itself is spawned at release.
            let shots = arsenal.grenade.step(dt, trigger, &aim);
            if !shots.is_empty() && arsenal.pending.is_none() {
                // The whole-body throw standing still, the upper-body one on the move or in the
                // air (ratchet_model picks the same).
                let v = p.sim.velocity() / settings.scale;
                let standing = p.sim.state() == MoveState::Ground && Vec2::new(v.x, v.z).length() <= 0.6;
                arsenal.throws += 1;
                arsenal.pending = Some(if standing { arsenal.grenade_release.0 } else { arsenal.grenade_release.1 });
            }
        }
        Slot::Wrench => {
            arsenal.pistol.step(dt, false, &aim);
            arsenal.grenade.step(dt, false, &aim);
        }
    }
    // Refill an empty weapon after a moment.
    let empty = match arsenal.slot {
        Slot::BurstPistol => arsenal.pistol.ammo() == 0,
        Slot::Shatterbomb => arsenal.grenade.ammo() == 0,
        Slot::Wrench => false,
    };
    arsenal.refill = if empty { arsenal.refill + dt } else { 0.0 };
    if arsenal.refill > REFILL_AFTER {
        arsenal.pistol.refill();
        arsenal.grenade.refill();
        arsenal.refill = 0.0;
    }
}

/// The weapon already spent the ammo when the throw started; at release, aim again from the
/// hand (this re-solves the lob for where the hand is now).
fn last_grenade_shot(arsenal: &Arsenal, aim: &Aim) -> Option<libarsenal::Shot> {
    let w = arsenal.grenade.params();
    let to = aim.target - aim.muzzle;
    let target = if to.length() > w.lob_range { aim.muzzle + to.normalize() * w.lob_range } else { aim.target };
    Some(libarsenal::Shot { origin: aim.muzzle, velocity: libarsenal::lob_velocity(aim.muzzle, target, w.projectile.speed, w.projectile.gravity) })
}

fn spawn_grenade(
    commands: &mut Commands,
    models: &WeaponModels,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    proj: Projectile,
    light: Option<PointLight>,
    scale: f32,
) {
    let at = Transform::from_translation(proj.position()).with_scale(Vec3::splat(scale));
    let mut e = commands.spawn((Grenade { proj }, at, Visibility::default()));
    if let Some(light) = light {
        e.with_child((light, Transform::default()));
    }
    match &models.grenade {
        Some(scene) => {
            e.with_children(|c| {
                c.spawn((WorldAssetRoot(scene.clone()), Transform::default()));
            });
        }
        None => {
            e.insert((
                Mesh3d(meshes.add(Sphere::new(0.12))),
                MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.8, 0.9), ..default() })),
            ));
        }
    }
}

/// Bolts: a hit on a dummy damages and nudges it; a wall or the floor stops them (with the
/// game's impact flare, sparks and a bullet mark). Each step leaves a stretch of smoke trail.
#[allow(clippy::too_many_arguments)]
fn fly_bolts(
    time: Res<Time>,
    settings: Res<Settings>,
    level: Res<Level>,
    art: Res<VfxArt>,
    mut rng: ResMut<FxRng>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Transform)>,
    mut dummies: Query<&mut Dummy>,
    mut hits: ResMut<Hits>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let s = settings.scale;
    for (e, mut bolt, mut tf) in &mut bolts {
        if bolt.spent {
            commands.entity(e).despawn();
            continue;
        }
        let (a, b) = bolt.proj.step(time.delta_secs());
        let r = bolt.proj.radius();
        // Sample the segment against the dummies.
        let steps = ((b - a).length() / 0.1).ceil().max(1.0) as usize;
        let mut impact: Option<(Vec3, Vec3, bool)> = None;
        'samples: for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            for mut d in dummies.iter_mut() {
                if dummy_contains(&d, p, r) {
                    let (damage, push) = (bolt.damage, bolt.knockback);
                    hit(&mut d, p - (b - a), push, 0.3, damage);
                    hits.count += 1;
                    hits.last = format!("Burst Pistol hit ({damage:.1} damage, dummy at {:.0})", d.health.max(0.0));
                    let axis = Vec3::new(d.pos.x, p.y, d.pos.z);
                    impact = Some((p, (p - axis).normalize_or(-(b - a).normalize_or(Vec3::Y)), false));
                    break 'samples;
                }
            }
        }
        if impact.is_none() {
            impact = level
                .blocks
                .iter()
                .filter_map(|blk| segment_hits_box(a, b, blk.min, blk.max))
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(t, n)| (a.lerp(b, t), n, true));
        }
        let end = impact.map_or(b, |(at, _, _)| at);
        if art.available {
            art.bolt_trail(&mut commands, &mut materials, a, end, s);
        }
        if let Some((at, normal, wall)) = impact {
            if art.available {
                art.bolt_impact(&mut commands, &mut materials, &mut rng, at, normal, wall, s);
                // Show it at the hit for a step (its streak catches up), then remove it.
                tf.translation = at;
                bolt.spent = true;
            } else {
                commands.entity(e).despawn();
            }
        } else if bolt.proj.expired() {
            commands.entity(e).despawn();
        } else {
            tf.translation = b;
        }
    }
}

/// Grenades: explode on the first thing they touch (or at the end of their life), damaging and
/// throwing every dummy in range by the explosion's falloff.
#[allow(clippy::too_many_arguments)]
fn fly_grenades(
    time: Res<Time>,
    settings: Res<Settings>,
    level: Res<Level>,
    arsenal: Res<Arsenal>,
    mut grenades: Query<(Entity, &mut Grenade, &mut Transform)>,
    mut dummies: Query<&mut Dummy>,
    mut hits: ResMut<Hits>,
    mut rig: ResMut<CameraRig>,
    shakes: Res<WeaponShakes>,
    players: Query<&Player>,
    art: Res<VfxArt>,
    mut rng: ResMut<FxRng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let Some(ex) = arsenal.grenade.params().explosion.clone() else { return };
    let player = players.single().map_or(Vec3::ZERO, |p| p.feet);
    for (e, mut g, mut tf) in &mut grenades {
        let (a, b) = g.proj.step(time.delta_secs());
        let r = g.proj.radius();
        let mut at = None;
        let steps = ((b - a).length() / 0.1).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            if dummies.iter().any(|d| dummy_contains(d, p, r)) {
                at = Some(p);
                break;
            }
        }
        if at.is_none() {
            at = level
                .blocks
                .iter()
                .filter_map(|blk| segment_hits_box(a, b, blk.min - Vec3::splat(r), blk.max + Vec3::splat(r)))
                .map(|(t, _)| t)
                .reduce(f32::min)
                .map(|t| a.lerp(b, t));
        }
        if at.is_none() && g.proj.expired() {
            at = Some(b);
        }
        let Some(at) = at else {
            tf.translation = b;
            tf.rotation = Quat::from_rotation_x(g.proj.spin());
            continue;
        };
        commands.entity(e).despawn();
        let mut n = 0;
        let mut total = 0.0;
        for mut d in dummies.iter_mut() {
            let dist = (d.pos + Vec3::Y * 0.6 * settings.scale).distance(at);
            if dist > ex.outer_radius {
                continue;
            }
            let damage = ex.damage_at(dist);
            let push = ex.knockback_at(dist);
            if d.health > 0.0 {
                hit(&mut d, at, push, push * 1.5, damage);
                n += 1;
                total += damage;
            }
        }
        hits.count += n;
        hits.last = format!("Shatterbomb hit {n} ({total:.0} damage in all, {:.1}-{:.1} m)", ex.inner_radius, ex.outer_radius);
        // The game's explosion shake, full strength near it and fading out further away.
        match &shakes.blast {
            Some((shake, inner, outer)) => {
                let d = player.distance(at);
                rig.add_shake(shake, if d <= *inner { 1.0 } else { ((outer - d) / (outer - inner).max(1e-3)).clamp(0.0, 1.0) });
            }
            None => rig.shake = 0.3,
        }
        if art.available {
            let ground = ground_distance(&level, at + Vec3::Y * 0.2, 0.05).filter(|d| *d < 1.5).map(|d| at.y + 0.2 - d);
            art.explosion(&mut commands, &mut materials, &mut rng, at, ex.outer_radius, ground, settings.scale);
            continue;
        }
        // The fireball (smaller than the damage radius, which is mostly shrapnel).
        commands.spawn((
            Blast { age: 0.0, radius: ex.outer_radius * 0.45, life: 0.4 },
            Mesh3d(meshes.add(Sphere::new(1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(1.0, 0.65, 0.2, 0.55),
                emissive: LinearRgba::rgb(10.0, 5.0, 1.0),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::from_translation(at).with_scale(Vec3::splat(0.2)),
        ));
    }
}

/// Explosion flash: grows to the outer radius and fades.
fn blasts(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Blast, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (e, mut b, mut tf, mat) in &mut q {
        b.age += time.delta_secs();
        let t = b.age / b.life;
        if t >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        tf.scale = Vec3::splat(b.radius * (0.3 + 0.7 * t.sqrt()));
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = Color::srgba(1.0, 0.65, 0.2, 0.55 * (1.0 - t));
        }
    }
}


/// The gun shows in the hand while the pistol is out (not during melee, when the wrench is).
fn gun_visibility(arsenal: Res<Arsenal>, players: Query<&Player>, mut vis: Query<&mut Visibility>) {
    let (Some(gun), Ok(p)) = (arsenal.gun, players.single()) else { return };
    let busy = matches!(p.sim.state(), MoveState::Melee | MoveState::Smash | MoveState::SmashLand);
    if let Ok(mut v) = vis.get_mut(gun) {
        v.set_if_neq(if arsenal.slot == Slot::BurstPistol && !busy { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// The pistol's own fire clip (the slide), restarted with each shot.
fn gun_clips(
    mut commands: Commands,
    mut arsenal: ResMut<Arsenal>,
    models: Res<WeaponModels>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut anim: Query<(&mut AnimationPlayer, Option<&mut AnimationTransitions>)>,
) {
    let Some(player) = arsenal.gun_player else { return };
    if arsenal.gun_fire.is_none() {
        let Some(gltf) = models.pistol.as_ref().and_then(|h| gltfs.get(h)) else { return };
        let Some(clip) = gltf.named_animations.get("fire") else { return };
        let mut graph = AnimationGraph::new();
        let node = graph.add_clip(clip.clone(), 1.0, graph.root);
        commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
        arsenal.gun_fire = Some(node);
        return;
    }
    if arsenal.shots != arsenal.shots_seen {
        arsenal.shots_seen = arsenal.shots;
        if let (Some(node), Ok((mut player, Some(mut transitions)))) = (arsenal.gun_fire, anim.get_mut(player)) {
            transitions.play(&mut player, node, Duration::ZERO);
        }
    }
}

/// Helper for other modules: the slot's display name and ammo.
pub fn hud_line(arsenal: &Arsenal) -> String {
    let (ammo, max, extra) = match arsenal.slot {
        Slot::Wrench => return "weapon: OmniWrench (1)  |  2 Burst Pistol, 3 Shatterbomb".into(),
        Slot::BurstPistol => {
            let w = &arsenal.pistol;
            (w.ammo(), w.params().ammo, format!("spread {:.2}°  |  {:.0}/s aimed, {:.0}/s from the hip", w.spread(), w.params().rate, w.params().rapid_rate))
        }
        Slot::Shatterbomb => {
            let w = &arsenal.grenade;
            let e = w.params().explosion.as_ref();
            (
                w.ammo(),
                w.params().ammo,
                format!(
                    "lobbed {:.0} m/s up to {:.0} m  |  blast {:.0} damage inside {:.1} m, {:.0} at {:.1} m",
                    w.params().projectile.speed,
                    w.params().lob_range,
                    e.map_or(0.0, |e| e.inner_damage),
                    e.map_or(0.0, |e| e.inner_radius),
                    e.map_or(0.0, |e| e.outer_damage),
                    e.map_or(0.0, |e| e.outer_radius)
                ),
            )
        }
    };
    let aim = if arsenal.aiming { "  AIMING" } else { "" };
    let note = if arsenal.note.is_empty() { String::new() } else { format!("  ({})", arsenal.note) };
    format!("weapon: {} {ammo}/{max}{aim}  |  {extra}{note}", arsenal.slot.name())
}


//! Target dummies for the wrench: the combo, the Hyper-Strike and the throw knock them back by
//! the distances from the melee config, then they walk back to their spot.

use crate::level::spots;
use crate::{CameraRig, Player, Settings, simulate};
use bevy::prelude::*;
use libratchet::Event as MoveEvent;

/// How far in front a swing reaches (ours; the game's hit volume isn't known).
const SWING_REACH: f32 = 1.8;
const DUMMY_RADIUS: f32 = 0.3;
const DUMMY_HEIGHT: f32 = 1.1;
/// Sliding friction after a knockback, so a hit travels its knockback distance.
const SLIDE_DECEL: f32 = 20.0;
const GRAVITY: f32 = 25.0;
/// Seconds a hit dummy stays put before walking home.
const STUN: f32 = 1.6;

#[derive(Component)]
pub struct Dummy {
    home: Vec3,
    pub pos: Vec3,
    vel: Vec3,
    /// Seconds until it walks home, and of the hit flash.
    stunned: f32,
    flash: f32,
    tilt: Vec3,
}

impl Dummy {
    /// Seconds since it was last hit (up to the stun time).
    pub fn since_hit(&self) -> f32 {
        STUN - self.stunned
    }
}

#[derive(Resource, Default)]
pub struct Hits {
    pub count: u32,
    pub last: String,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Hits>()
        .add_systems(Startup, spawn_dummies.after(crate::level::spawn_level))
        .add_systems(FixedUpdate, (melee_hits.after(simulate), dummy_physics).chain())
        .add_systems(Update, dummy_visuals);
}

/// Dummies in the arena, Ratchet starting at `spots::ARENA` facing +X: three within the first
/// swing, a ring for the Hyper-Strike 8 m on, and one far out for the throw.
pub const DUMMY_SPOTS: &[(f32, f32)] = &[(1.6, 0.0), (1.5, 0.9), (1.5, -0.9), (9.5, 0.0), (8.0, 1.4), (8.0, -1.4), (6.6, 0.6), (22.0, 4.0)];

fn spawn_dummies(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let body = meshes.add(Capsule3d::new(DUMMY_RADIUS, DUMMY_HEIGHT - 2.0 * DUMMY_RADIUS));
    let band = meshes.add(Cylinder::new(DUMMY_RADIUS * 1.05, 0.12));
    let base = meshes.add(Cylinder::new(0.4, 0.08));
    let red = materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.15, 0.15), ..default() });
    let grey = materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.3, 0.35), ..default() });
    for &(x, z) in DUMMY_SPOTS {
        let home = spots::ARENA + Vec3::new(x, 0.0, z);
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.75, 0.72, 0.62), perceptual_roughness: 0.7, ..default() });
        commands
            .spawn((
                Dummy { home, pos: home, vel: Vec3::ZERO, stunned: 0.0, flash: 0.0, tilt: Vec3::ZERO },
                Transform::from_translation(home),
                Visibility::default(),
            ))
            .with_children(|d| {
                d.spawn((Mesh3d(body.clone()), MeshMaterial3d(mat), Transform::from_xyz(0.0, DUMMY_HEIGHT * 0.5 + 0.08, 0.0)));
                d.spawn((Mesh3d(band.clone()), MeshMaterial3d(red.clone()), Transform::from_xyz(0.0, DUMMY_HEIGHT * 0.62, 0.0)));
                d.spawn((Mesh3d(base.clone()), MeshMaterial3d(grey.clone()), Transform::from_xyz(0.0, 0.04, 0.0)));
            });
    }
}

/// Knocks every dummy within `radius` of `center` (and passing `filter`) away from `from`, so it
/// slides `knockback` metres; `lift` pops it up. Returns how many were hit.
pub fn knock(dummies: &mut Query<&mut Dummy>, center: Vec3, radius: f32, from: Vec3, knockback: f32, lift: f32, filter: impl Fn(&Dummy) -> bool) -> usize {
    let mut n = 0;
    for mut d in dummies.iter_mut() {
        let flat = Vec3::new(d.pos.x - center.x, 0.0, d.pos.z - center.z);
        if flat.length() > radius + DUMMY_RADIUS || !filter(&d) {
            continue;
        }
        let away = Vec3::new(d.pos.x - from.x, 0.0, d.pos.z - from.z).normalize_or(Vec3::X);
        let speed = (2.0 * SLIDE_DECEL * knockback).sqrt();
        d.vel = away * speed + Vec3::Y * lift;
        d.stunned = STUN;
        d.flash = 0.15;
        n += 1;
    }
    n
}

/// Applies the sim's melee events: swing hits in front of Ratchet, the Hyper-Strike all around.
fn melee_hits(settings: Res<Settings>, players: Query<&Player>, mut dummies: Query<&mut Dummy>, mut hits: ResMut<Hits>, mut rig: ResMut<CameraRig>) {
    let Ok(p) = players.single() else { return };
    let s = settings.scale;
    let t = p.sim.tuning().melee.clone();
    for e in p.sim.events() {
        match *e {
            MoveEvent::MeleeHit { index } => {
                let w = &t.combo[index];
                let facing = p.sim.facing_dir();
                let reach = SWING_REACH * s;
                let center = p.feet + facing * reach * 0.5;
                let feet = p.feet;
                let n = knock(&mut dummies, center, reach * 0.6, feet - facing * 0.5 * s, w.knockback, 1.5 * s, |d| {
                    let to = Vec3::new(d.pos.x - feet.x, 0.0, d.pos.z - feet.z);
                    to.length() <= reach + DUMMY_RADIUS && to.normalize_or_zero().dot(facing) > 0.2
                });
                if n > 0 {
                    hits.count += n as u32;
                    hits.last = format!("swing {} hit {n} ({} damage each, knocked back {:.1} m)", index + 1, w.damage, w.knockback);
                }
            }
            MoveEvent::SmashImpact { .. } => {
                rig.shake = 0.25;
                let n = knock(&mut dummies, p.feet, t.smash_radius, p.feet, t.smash_radius, 6.0 * s, |_| true);
                hits.count += n as u32;
                hits.last = format!("Hyper-Strike hit {n} ({} damage each, {:.1} m around)", t.smash_damage, t.smash_radius);
            }
            _ => {}
        }
    }
}

fn dummy_physics(time: Res<Time>, mut dummies: Query<(&mut Dummy, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut d, mut tf) in &mut dummies {
        d.stunned = (d.stunned - dt).max(0.0);
        d.flash = (d.flash - dt).max(0.0);
        let airborne = d.pos.y > 0.0 || d.vel.y > 0.0;
        if airborne {
            d.vel.y -= GRAVITY * dt;
        } else {
            let flat = Vec2::new(d.vel.x, d.vel.z);
            let slowed = flat.normalize_or_zero() * (flat.length() - SLIDE_DECEL * dt).max(0.0);
            d.vel = Vec3::new(slowed.x, 0.0, slowed.y);
            if d.stunned <= 0.0 && flat.length() < 0.05 {
                // Walk back home.
                let to = d.home - d.pos;
                let step = (2.0 * dt).min(to.length());
                let pos = d.pos + to.normalize_or_zero() * step;
                d.pos = pos;
            }
        }
        let vel = d.vel;
        d.pos += vel * dt;
        if d.pos.y < 0.0 {
            d.pos.y = 0.0;
            d.vel.y = 0.0;
        }
        // Lean with the knockback.
        let lean = Vec3::new(d.vel.z, 0.0, -d.vel.x) * 0.06;
        d.tilt = d.tilt.lerp(lean, (12.0 * dt).min(1.0));
        tf.translation = d.pos;
        tf.rotation = if d.tilt.length() > 1e-4 { Quat::from_axis_angle(d.tilt.normalize(), d.tilt.length().min(1.2)) } else { Quat::IDENTITY };
    }
}

/// Hit flash: the body glows for a moment.
fn dummy_visuals(
    dummies: Query<(&Dummy, &Children)>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (d, children) in &dummies {
        let Some(&body) = children.first() else { continue };
        if let Ok(handle) = meshes.get(body)
            && let Some(mut m) = materials.get_mut(&handle.0)
        {
            m.emissive = if d.flash > 0.0 { LinearRgba::rgb(6.0, 4.0, 1.0) } else { LinearRgba::BLACK };
        }
    }
}

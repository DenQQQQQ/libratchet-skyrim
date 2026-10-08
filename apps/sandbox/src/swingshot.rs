//! The Swingshot in the sandbox: the level's targets as orbs (the one he'd fire at lights up)
//! and the cable from his swingshot hand, out to the shot and back.

use crate::level::Level;
use crate::{Player, Settings};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

/// How fast the shot flies out to the target and back to his hand (m/s at Ratchet's size;
/// RiftApart.exe).
const SHOT_SPEED: f32 = 80.0;

/// A target's orb, at the target.
#[derive(Component)]
struct Orb(Vec3);

#[derive(Component)]
struct Cable;

/// The shot at the cable's end (RiftApart.exe): hooking on fires it from his hand toward the
/// target's attach point at SHOT_SPEED, plus the target's own motion; there it stays while he's
/// on. Let go (however he lets go), it flies back to his hand at the same speed, and the cable is
/// put away once it's there. Firing again while it's on its way back finishes that at once: the
/// new shot leaves from his hand. The swing itself never waits for it.
#[derive(Resource, Default)]
struct Shot {
    /// Where the shot is, while the cable is out.
    head: Option<Vec3>,
    /// The target it's out to, as of the last frame (none on its way back).
    target: Option<Vec3>,
    /// The player's shot and respawn counts this has seen.
    shots_seen: u32,
    respawns_seen: u32,
}

#[derive(Resource)]
struct Looks {
    idle: Handle<StandardMaterial>,
    picked: Handle<StandardMaterial>,
}

/// His swingshot hand (the model's igLoc_swingshot locator), once the model is in. Bevy can spawn
/// a model's scene again after it loads, so the newest locator wins.
#[derive(Resource, Default)]
struct Hand(Option<Entity>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Hand>()
        .init_resource::<Shot>()
        .add_systems(Startup, setup.after(crate::level::spawn_level))
        .add_systems(Update, (find_hand, light_orbs))
        // After this frame's transforms, so the cable leaves his hand where it is now, not where
        // it was a frame ago (0.4 m behind at full swing).
        .add_systems(PostUpdate, draw_cable.after(TransformSystems::Propagate));
}

fn setup(mut commands: Commands, level: Res<Level>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let idle = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.55, 1.0), emissive: LinearRgba::rgb(0.3, 0.8, 2.0), ..default() });
    let picked = materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.6, 0.1), emissive: LinearRgba::rgb(3.0, 1.5, 0.3), ..default() });
    let hook = materials.add(StandardMaterial { base_color: Color::srgb(0.3, 1.0, 0.4), emissive: LinearRgba::rgb(0.4, 2.0, 0.6), ..default() });
    let orb = meshes.add(Sphere::new(0.3));
    for t in &level.swing_targets {
        let look = if t.kind == libratchet::SwingKind::Hook { hook.clone() } else { idle.clone() };
        commands.spawn((Mesh3d(orb.clone()), MeshMaterial3d(look), Transform::from_translation(t.position), Orb(t.position)));
    }
    let cable = materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.9, 1.0), emissive: LinearRgba::rgb(0.8, 1.0, 1.6), ..default() });
    commands.spawn((Mesh3d(meshes.add(Cylinder::new(0.02, 1.0))), MeshMaterial3d(cable), Transform::default(), Visibility::Hidden, Cable));
    commands.insert_resource(Looks { idle, picked });
}

fn find_hand(mut hand: ResMut<Hand>, names: Query<(Entity, &Name), Added<Name>>) {
    if let Some((e, _)) = names.iter().find(|(_, n)| n.as_str() == "igLoc_swingshot") {
        hand.0 = Some(e);
    }
}

/// The target he'd fire at (or is on) glows orange; hook targets stay green.
fn light_orbs(players: Query<&Player>, level: Res<Level>, looks: Res<Looks>, mut orbs: Query<(&Orb, &mut MeshMaterial3d<StandardMaterial>)>) {
    let Ok(p) = players.single() else { return };
    let on = p.sim.swinging_on().map(|(t, _)| t).or(p.sim.swing_target());
    for (orb, mut look) in &mut orbs {
        let hook = level.swing_targets.iter().any(|t| t.position == orb.0 && t.kind == libratchet::SwingKind::Hook);
        if hook {
            continue;
        }
        let want = if on == Some(orb.0) { &looks.picked } else { &looks.idle };
        if look.0 != *want {
            look.0 = want.clone();
        }
    }
}

/// The cable, from his hand to the shot (see `Shot`).
fn draw_cable(
    time: Res<Time>,
    settings: Res<Settings>,
    players: Query<&Player>,
    hand: Res<Hand>,
    mut shot: ResMut<Shot>,
    globals: Query<&GlobalTransform, Without<Cable>>,
    mut cable: Query<(&mut Transform, &mut GlobalTransform, &mut Visibility), With<Cable>>,
) {
    let (Ok(p), Ok((mut tf, mut global, mut vis))) = (players.single(), cable.single_mut()) else { return };
    let from = hand.0.and_then(|h| globals.get(h).ok()).map_or(p.feet + Vec3::Y, |g| g.translation());
    if p.respawns != shot.respawns_seen {
        shot.respawns_seen = p.respawns;
        shot.head = None;
    }
    if p.shots != shot.shots_seen {
        shot.shots_seen = p.shots;
        shot.head = Some(from);
        shot.target = None;
    }
    let step = SHOT_SPEED * settings.scale * time.delta_secs();
    let on = p.sim.swinging_on().map(|(t, _)| t);
    shot.head = match (shot.head, on) {
        // Out to the target (it moves with it), or there.
        (Some(head), Some(target)) => Some((head + shot.target.map_or(Vec3::ZERO, |was| target - was)).move_towards(target, step)),
        // Back to his hand.
        (Some(head), None) => Some(head.move_towards(from, step)).filter(|h| h.distance(from) > 1e-3),
        (None, _) => None,
    };
    shot.target = on;
    let Some(head) = shot.head else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    let along = head - from;
    let length = along.length();
    if length < 1e-3 {
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    *tf = Transform {
        translation: (from + head) * 0.5,
        rotation: Quat::from_rotation_arc(Vec3::Y, along / length),
        scale: Vec3::new(1.0, length, 1.0),
    };
    // Transforms are already propagated this frame; the cable has no parent, so its global
    // transform is its own.
    *global = GlobalTransform::from(*tf);
    vis.set_if_neq(Visibility::Inherited);
}

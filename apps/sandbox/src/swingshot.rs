//! The Swingshot in the sandbox: the level's targets as orbs (the one he'd fire at lights up)
//! and the cable from his swingshot hand while he swings or reels.

use crate::level::Level;
use crate::Player;
use bevy::prelude::*;

/// A target's orb, at the target.
#[derive(Component)]
struct Orb(Vec3);

#[derive(Component)]
struct Cable;

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
        .add_systems(Startup, setup.after(crate::level::spawn_level))
        .add_systems(Update, (find_hand, light_orbs, draw_cable));
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

/// The cable, from his hand to the target, while he's on one.
fn draw_cable(players: Query<&Player>, hand: Res<Hand>, globals: Query<&GlobalTransform>, mut cable: Query<(&mut Transform, &mut Visibility), With<Cable>>) {
    let (Ok(p), Ok((mut tf, mut vis))) = (players.single(), cable.single_mut()) else { return };
    let Some((target, _)) = p.sim.swinging_on() else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    let from = hand.0.and_then(|h| globals.get(h).ok()).map_or(p.feet + Vec3::Y, |g| g.translation());
    let along = target - from;
    let length = along.length();
    if length < 1e-3 {
        return;
    }
    *tf = Transform {
        translation: (from + target) * 0.5,
        rotation: Quat::from_rotation_arc(Vec3::Y, along / length),
        scale: Vec3::new(1.0, length, 1.0),
    };
    vis.set_if_neq(Visibility::Inherited);
}

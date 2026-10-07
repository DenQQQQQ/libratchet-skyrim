//! Shows Ratchet's real model on the player when `riftapart-assets/ratchet.glb` exists (made
//! from the user's own Rift Apart install by `tools/extract_riftapart_ratchet.py`). When the file
//! carries the game's animation clips (idle, run, jump...), they're played by move state with
//! crossfades; otherwise the skeleton is posed procedurally.
//!
//! Bone rotations are given in the model's rest-pose space and applied as
//! `local = parentRest⁻¹ · R · parentRest · localRest`, which bends each joint around an axis
//! that stays attached to its parent (a proper hinge), whatever the rig's local axes are.

use crate::{Player, Settings};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use libratchet::MoveState;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;
use std::time::Duration;

pub const MODEL_FILE: &str = "ratchet.glb";

/// Finds `riftapart-assets/` (with the converted model) in the working directory, next to the
/// executable, or any folder above them.
pub fn find_assets_dir() -> Option<PathBuf> {
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from))];
    for start in starts.into_iter().flatten() {
        let mut dir = start;
        loop {
            let cand = dir.join("riftapart-assets");
            if cand.join(MODEL_FILE).exists() {
                return Some(cand);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

/// Marks the capsule placeholder parts, hidden once the real model is in.
#[derive(Component)]
pub struct Placeholder;

#[derive(Component)]
struct ModelRoot;

#[derive(Resource)]
struct ModelGltf(Handle<Gltf>);

/// Clip names baked into ratchet.glb by the converter (one per move).
const CLIP_STATES: &[&str] = &[
    "idle", "run", "jump", "double_jump", "fall", "glide", "land", "dash", "air_dash", "melee_1", "melee_2", "melee_3", "smash_intro", "smash",
    "throw",
];
const MELEE_CLIPS: &[&str] = &["melee_1", "melee_2", "melee_3"];
const LOOPING: &[&str] = &["idle", "run", "fall", "glide"];

struct Bone {
    entity: Entity,
    rest_local: Quat,
    parent_rest: Quat,
}

#[derive(Resource)]
pub struct Rig {
    bones: HashMap<&'static str, Bone>,
    /// Rest-pose angle of each upper arm in the model's XY plane (radians), to lower the arms
    /// from whatever pose the model was authored in.
    arm_rest_angle: [f32; 2],
    pose: Pose,
    phase: f32,
    /// Entity with the glTF AnimationPlayer (the skeleton root), when the model has clips.
    player: Option<Entity>,
    clips: HashMap<&'static str, AnimationNodeIndex>,
    current_clip: Option<&'static str>,
    prev_state: Option<MoveState>,
    land_timer: f32,
}

const BONES: &[&str] = &[
    "pelvis", "spine_1", "spine_2", "spine_3", "neck_01", "head", "LF_upleg", "RT_upleg", "LF_loleg", "RT_loleg", "LF_uparm", "RT_uparm",
    "LF_loarm", "RT_loarm", "LF_ear_1", "RT_ear_1", "tail_1", "tail_2", "tail_3", "tail_4", "tail_5", "tail_6",
];

/// Pose parameters in radians. Legs: + swings back. Spine: + leans forward.
/// Arm drop: 0 = straight out to the side, π/2 = straight down.
#[derive(Clone, Copy, Default)]
struct Pose {
    hip: [f32; 2],
    knee: [f32; 2],
    arm_drop: f32,
    arm_swing: [f32; 2],
    elbow: f32,
    lean: f32,
    head: f32,
    ears: f32,
    tail_lift: f32,
    tail_sway: f32,
}

impl Pose {
    fn lerp(&self, o: &Pose, t: f32) -> Pose {
        let l = |a: f32, b: f32| a + (b - a) * t;
        Pose {
            hip: [l(self.hip[0], o.hip[0]), l(self.hip[1], o.hip[1])],
            knee: [l(self.knee[0], o.knee[0]), l(self.knee[1], o.knee[1])],
            arm_drop: l(self.arm_drop, o.arm_drop),
            arm_swing: [l(self.arm_swing[0], o.arm_swing[0]), l(self.arm_swing[1], o.arm_swing[1])],
            elbow: l(self.elbow, o.elbow),
            lean: l(self.lean, o.lean),
            head: l(self.head, o.head),
            ears: l(self.ears, o.ears),
            tail_lift: l(self.tail_lift, o.tail_lift),
            tail_sway: l(self.tail_sway, o.tail_sway),
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (setup_clips, play_clips, animate).chain().after(crate::update_visuals));
}

/// Spawns the model under the player. Called from setup when the model file exists.
pub fn spawn(commands: &mut Commands, assets: &AssetServer, player: Entity) {
    commands.insert_resource(ModelGltf(assets.load(MODEL_FILE)));
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL_FILE));
    let model = commands.spawn((ModelRoot, WorldAssetRoot(scene), Transform::default())).observe(on_ready).id();
    commands.entity(player).add_child(model);
}

fn on_ready(
    ev: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    nodes: Query<(&Transform, Option<&Name>)>,
    parents: Query<&ChildOf>,
    placeholders: Query<Entity, With<Placeholder>>,
    players: Query<(), With<AnimationPlayer>>,
    assets: Res<AssetServer>,
    settings: Res<Settings>,
) {
    let root = ev.entity;
    let player = children.iter_descendants(root).find(|e| players.contains(*e));
    // Name -> entity for everything under the model.
    let mut by_name = HashMap::new();
    for e in children.iter_descendants(root) {
        if let Ok((_, Some(name))) = nodes.get(e) {
            by_name.entry(name.as_str().to_string()).or_insert(e);
        }
    }
    // Rest-pose rotation/translation of an entity in model space.
    let rest_global = |mut e: Entity| -> (Quat, Vec3) {
        let mut chain = vec![];
        while e != root {
            chain.push(e);
            match parents.get(e) {
                Ok(p) => e = p.parent(),
                Err(_) => break,
            }
        }
        let (mut rot, mut pos) = (Quat::IDENTITY, Vec3::ZERO);
        for e in chain.into_iter().rev() {
            if let Ok((t, _)) = nodes.get(e) {
                pos += rot * t.translation;
                rot *= t.rotation;
            }
        }
        (rot, pos)
    };

    // Clank rides on the ClankAttach bone, like in the game.
    if let (Some(dir), Some(&attach)) = (settings.model_dir.as_ref(), by_name.get("ClankAttach"))
        && dir.join(crate::clank::MODEL_FILE).exists()
    {
        crate::clank::spawn(&mut commands, &assets, attach);
    }
    // The OmniWrench goes in his right hand: the game mounts weapons on the igLoc_RT_gunAttach
    // locator, which sits on RT_handle_bound with no offset (the wrench's origin is its grip).
    if let (Some(dir), Some(&hand)) = (settings.model_dir.as_ref(), by_name.get("RT_handle_bound"))
        && dir.join(crate::wrench::MODEL_FILE).exists()
    {
        crate::wrench::spawn(&mut commands, &assets, hand);
    }

    let mut bones = HashMap::new();
    for &name in BONES {
        let Some(&e) = by_name.get(name) else {
            warn!("ratchet model: bone {name} not found");
            continue;
        };
        let parent_rest = parents.get(e).map(|p| rest_global(p.parent()).0).unwrap_or(Quat::IDENTITY);
        let rest_local = nodes.get(e).map(|(t, _)| t.rotation).unwrap_or(Quat::IDENTITY);
        bones.insert(name, Bone { entity: e, rest_local, parent_rest });
    }
    let arm_angle = |up: &str, lo: &str| -> f32 {
        match (by_name.get(up), by_name.get(lo)) {
            (Some(&u), Some(&l)) => {
                let d = rest_global(l).1 - rest_global(u).1;
                d.y.atan2(d.x)
            }
            _ => 0.0,
        }
    };
    let arm_rest_angle = [arm_angle("LF_uparm", "LF_loarm"), arm_angle("RT_uparm", "RT_loarm")];
    info!("ratchet model ready: {} bones rigged, arm rest angles {:.0}°/{:.0}°", bones.len(), arm_rest_angle[0].to_degrees(), arm_rest_angle[1].to_degrees());
    commands.insert_resource(Rig {
        bones,
        arm_rest_angle,
        pose: Pose::default(),
        phase: 0.0,
        player,
        clips: HashMap::new(),
        current_clip: None,
        prev_state: None,
        land_timer: 0.0,
    });
    for e in &placeholders {
        commands.entity(e).insert(Visibility::Hidden);
    }
}

/// Builds the animation graph once the glTF (with its named clips) is loaded.
fn setup_clips(
    mut commands: Commands,
    rig: Option<ResMut<Rig>>,
    model: Option<Res<ModelGltf>>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let (Some(mut rig), Some(model)) = (rig, model) else { return };
    let Some(player) = rig.player else { return };
    if !rig.clips.is_empty() {
        return;
    }
    let Some(gltf) = gltfs.get(&model.0) else { return };
    let mut graph = AnimationGraph::new();
    let mut clips = HashMap::new();
    for &name in CLIP_STATES {
        if let Some(clip) = gltf.named_animations.get(name) {
            clips.insert(name, graph.add_clip(clip.clone(), 1.0, graph.root));
        }
    }
    if clips.is_empty() {
        rig.player = None; // no clips in this file: stay procedural
        return;
    }
    info!("ratchet model: playing {} Rift Apart clips", clips.len());
    commands.entity(player).insert((AnimationGraphHandle(graphs.add(graph)), AnimationTransitions::new()));
    rig.clips = clips;
}

/// Picks the clip for the current move and crossfades to it.
fn play_clips(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Option<ResMut<Rig>>,
    wrench: Res<crate::wrench::Wrench>,
    players: Query<&Player>,
    mut anim: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let (Some(mut rig), Ok(p)) = (rig, players.single()) else { return };
    let Some(entity) = rig.player else { return };
    if rig.clips.is_empty() {
        return;
    }
    let Ok((mut player, mut transitions)) = anim.get_mut(entity) else { return };
    let v = p.sim.velocity() / settings.scale;
    let speed = Vec2::new(v.x, v.z).length();
    let state = p.sim.state();
    let airborne = |s: Option<MoveState>| matches!(s, Some(MoveState::Jump | MoveState::DoubleJump | MoveState::Fall | MoveState::Glide | MoveState::AirDash));
    if state == MoveState::Ground && airborne(rig.prev_state) {
        rig.land_timer = 0.3;
    }
    rig.prev_state = Some(state);
    rig.land_timer = (rig.land_timer - time.delta_secs()).max(0.0);

    let want: &'static str = match state {
        MoveState::Ground if wrench.throw_anim > 0.0 && speed <= 0.6 => "throw",
        MoveState::Ground if speed > 0.6 => "run",
        MoveState::Ground if rig.land_timer > 0.0 => "land",
        MoveState::Ground => "idle",
        MoveState::Jump if v.y < -5.0 => "fall",
        MoveState::Jump => "jump",
        MoveState::DoubleJump => "double_jump",
        MoveState::Fall => "fall",
        MoveState::Glide => "glide",
        MoveState::Dash => "dash",
        MoveState::AirDash => "air_dash",
        MoveState::Melee => MELEE_CLIPS[p.sim.melee_swing().unwrap_or(0).min(MELEE_CLIPS.len() - 1)],
        MoveState::Smash => "smash_intro",
        MoveState::SmashLand => "smash",
    };
    let want = if rig.clips.contains_key(want) { want } else { "idle" };
    if rig.current_clip != Some(want)
        && let Some(&node) = rig.clips.get(want)
    {
        // Wrench moves cut in quickly.
        let quick = |c: Option<&str>| c.is_some_and(|c| MELEE_CLIPS.contains(&c) || c.starts_with("smash"));
        let fade = if quick(Some(want)) || quick(rig.current_clip) {
            60
        } else if want == "land" || rig.current_clip == Some("land") {
            90
        } else {
            160
        };
        let active = transitions.play(&mut player, node, Duration::from_millis(fade));
        if LOOPING.contains(&want) {
            active.repeat();
        }
        rig.current_clip = Some(want);
    }
    // Match the run cycle to the actual speed.
    if want == "run"
        && let Some(&node) = rig.clips.get("run")
        && let Some(active) = player.animation_mut(node)
    {
        let run_max = p.sim.tuning().ground_speeds.run_max / settings.scale;
        active.set_speed((speed / run_max.max(0.1)).clamp(0.5, 1.5));
    }
}

fn target_pose(state: MoveState, speed: f32, vy: f32, run_max: f32, phase: f32, t: f32) -> Pose {
    let s = (speed / run_max.max(0.1)).clamp(0.0, 1.3);
    let falling = (-vy / 10.0).clamp(0.0, 1.0);
    let sway = (t * 1.7).sin();
    match state {
        MoveState::Ground if speed > 0.25 => {
            let (a, b) = (phase.sin(), (phase + PI).sin());
            Pose {
                hip: [-0.65 * s * a, -0.65 * s * b],
                knee: [0.15 + 1.0 * s * (phase + FRAC_PI_2).sin().max(0.0), 0.15 + 1.0 * s * (phase + PI + FRAC_PI_2).sin().max(0.0)],
                arm_drop: 1.2,
                arm_swing: [0.7 * s * a, 0.7 * s * b],
                elbow: 0.4 + 0.6 * s,
                lean: 0.28 * s,
                head: -0.15 * s,
                ears: 0.35 * s,
                tail_lift: -0.25 * s,
                tail_sway: 0.35 * s * (phase * 0.5).sin(),
            }
        }
        MoveState::Ground => Pose {
            hip: [0.0; 2],
            knee: [0.08; 2],
            arm_drop: 1.25,
            arm_swing: [0.05, 0.05],
            elbow: 0.25,
            lean: 0.04 + 0.02 * (t * 2.0).sin(),
            head: 0.0,
            ears: 0.05 * (t * 0.9).sin(),
            tail_lift: 0.0,
            tail_sway: 0.25 * sway,
        },
        MoveState::Jump => Pose {
            hip: [-0.9, -0.3],
            knee: [1.3, 0.5],
            arm_drop: 0.7,
            arm_swing: [-0.5, -0.5],
            elbow: 0.8,
            lean: 0.1,
            head: -0.1,
            ears: 0.6 * falling,
            tail_lift: 0.3,
            tail_sway: 0.1 * sway,
        },
        MoveState::DoubleJump => Pose {
            hip: [-1.4, -1.2],
            knee: [2.0, 1.9],
            arm_drop: 0.9,
            arm_swing: [-0.9, -0.9],
            elbow: 1.3,
            lean: 0.45,
            head: 0.1,
            ears: 0.7,
            tail_lift: 0.6,
            tail_sway: 0.0,
        },
        MoveState::Fall => Pose {
            hip: [-0.3, 0.2],
            knee: [0.6, 0.3],
            arm_drop: 0.2,
            arm_swing: [-0.3, -0.2],
            elbow: 0.5,
            lean: -0.05,
            head: -0.2,
            ears: -0.6 * falling,
            tail_lift: 0.5,
            tail_sway: 0.3 * sway,
        },
        MoveState::Glide => Pose {
            hip: [0.45, 0.45],
            knee: [0.5, 0.5],
            arm_drop: -0.05,
            arm_swing: [0.25, 0.25],
            elbow: 0.1,
            lean: 0.35,
            head: -0.35,
            ears: -0.5,
            tail_lift: -0.2,
            tail_sway: 0.15 * sway,
        },
        MoveState::Dash | MoveState::AirDash | MoveState::Melee | MoveState::Smash | MoveState::SmashLand => Pose {
            hip: [-0.7, 0.8],
            knee: [0.9, 0.6],
            arm_drop: 1.1,
            arm_swing: [1.0, 1.0],
            elbow: 0.3,
            lean: 0.55,
            head: -0.45,
            ears: -0.8,
            tail_lift: -0.5,
            tail_sway: 0.0,
        },
    }
}

fn animate(
    time: Res<Time>,
    settings: Res<Settings>,
    rig: Option<ResMut<Rig>>,
    players: Query<&Player>,
    mut transforms: Query<&mut Transform>,
) {
    let (Some(mut rig), Ok(p)) = (rig, players.single()) else { return };
    if !rig.clips.is_empty() {
        return; // the game's own clips are playing
    }
    let dt = time.delta_secs();
    let v = p.sim.velocity() / settings.scale;
    let speed = Vec2::new(v.x, v.z).length();
    let run_max = p.sim.tuning().ground_speeds.run_max / settings.scale;
    // About 1.5 m per stride at Ratchet's size.
    rig.phase = (rig.phase + dt * speed / 1.5 * TAU) % (TAU * 2.0);
    let target = target_pose(p.sim.state(), speed, v.y, run_max, rig.phase, time.elapsed_secs());
    let k = 1.0 - (-14.0 * dt).exp();
    rig.pose = rig.pose.lerp(&target, k);
    let pose = rig.pose;
    let [rest_l, rest_r] = rig.arm_rest_angle;

    let mut set = |name: &str, r: Quat| {
        if let Some(b) = rig.bones.get(name)
            && let Ok(mut t) = transforms.get_mut(b.entity)
        {
            t.rotation = b.parent_rest.inverse() * r * b.parent_rest * b.rest_local;
        }
    };
    let x = Quat::from_rotation_x;
    let y = Quat::from_rotation_y;
    let z = Quat::from_rotation_z;

    for (i, side) in ["LF", "RT"].into_iter().enumerate() {
        set(&format!("{side}_upleg"), x(pose.hip[i]));
        set(&format!("{side}_loleg"), x(pose.knee[i]));
        // Left arm points +X, right arm −X. Drop angle below horizontal, then swing.
        let target_angle = if i == 0 { -pose.arm_drop } else { pose.arm_drop - PI };
        let rest = if i == 0 { rest_l } else { rest_r };
        let mut drop = target_angle - rest;
        while drop > PI {
            drop -= TAU;
        }
        while drop < -PI {
            drop += TAU;
        }
        set(&format!("{side}_uparm"), x(-pose.arm_swing[i]) * z(drop));
        set(&format!("{side}_loarm"), y(if i == 0 { -pose.elbow } else { pose.elbow }));
        set(&format!("{side}_ear_1"), x(-pose.ears));
    }
    for s in ["spine_1", "spine_2", "spine_3"] {
        set(s, x(pose.lean / 3.0));
    }
    set("neck_01", x(pose.head * 0.5));
    set("head", x(pose.head * 0.5));
    for (i, s) in ["tail_1", "tail_2", "tail_3", "tail_4", "tail_5", "tail_6"].into_iter().enumerate() {
        let f = 1.0 + i as f32 * 0.3;
        set(s, y(pose.tail_sway * 0.25 * f) * x(pose.tail_lift * 0.15));
    }
}

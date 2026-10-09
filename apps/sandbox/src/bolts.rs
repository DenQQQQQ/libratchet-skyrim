//! Rift Apart's bolts, the currency, through libbolts. A combat dummy knocked down drops the
//! game's test dummy's reward from 1 m up: 20 bolts (its RelativeValue of 2 at the default level
//! multiplier of 10). They burst out, bounce and come to rest, and swarm into Ratchet once he
//! (or his thrown wrench) is within a pickup's range, each with the game's collect sound; the
//! game has no sound for a drop or a bounce. The counter at the top right is the game's: gains
//! wait as "+N" until 2 s after the last, and it shows for 3.5 s after a change.
//!
//! The pickups wear the game's twelve models when `riftapart-assets/bolt_<tier>_0<n>.glb` exist
//! (`tools/extract_riftapart_ratchet.py --character bolts`), and twinkle with its glint when
//! `vfx/bolt_glint.png` does; without them, small rings.
//!
//! Not played: the game's approach loop (`bolt_approach`). Only bronze bolts have it, starting
//! 15 m out, past their 10 m pickup range, so it isn't the sound of the swarm, and what it's
//! for isn't clear.

use crate::combat::Dummy;
use crate::level::{Level, raycast};
use crate::sound::SoundCue;
use crate::{Player, Settings, simulate};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};
use libbolts::{BoltParams, BoltWorld, DEFAULT_LEVEL_MULTIPLIER, Hero, Hit, Kind, Rng, State, Tally, Tier};
use std::collections::{HashMap, HashSet};

/// The game's test dummy (`enm_testdummy_ps4`) is worth this (its RelativeValue, RiftApart.exe).
const DUMMY_VALUE: f32 = 2.0;
/// Bolts burst out this far above an enemy's feet, when it has no spawn locator (RiftApart.exe).
const DROP_HEIGHT: f32 = 1.0;
/// The bolts' randomness starts here (ours), so demo runs repeat.
const SEED: u32 = 0xB017;
/// The glint: a twinkle this long and this wide (metres at Ratchet's size), every so often
/// (seconds), held this far toward the camera so the bolt doesn't hide it. All ours: the game's
/// effect timing isn't decoded.
const GLINT_LIFE: f32 = 0.35;
const GLINT_SIZE: f32 = 0.3;
const GLINT_EVERY: (f32, f32) = (0.8, 2.5);
const GLINT_OUT: f32 = 0.12;
/// The glint's size over its life: grow, hold, shrink away (the game's curve for it, as we read
/// its keys: time, size).
const GLINT_CURVE: [(f32, f32); 6] = [(0.0, 0.5), (0.186, 1.0), (0.399, 1.0), (0.625, 0.467), (0.796, 0.172), (1.0, 0.0)];

/// The bolts in the level, and Ratchet's count of them.
#[derive(Resource)]
pub struct Bolts {
    world: BoltWorld,
    tally: Tally,
    /// Bolts dropped so far.
    dropped: u32,
    /// Ratchet's size the numbers are at.
    scale: f32,
    /// Each pickup's entity, by its id.
    drawn: HashMap<u32, Entity>,
    /// The game's models (by tier, three looks each), or the stand-ins' mesh and materials.
    models: Option<Vec<Handle<WorldAsset>>>,
    ring: (Handle<Mesh>, Vec<Handle<StandardMaterial>>),
    glint: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// For the twinkles' timing.
    rng: Rng,
    /// Some of the bolts were flying last tick (for the landing log).
    flying: bool,
}

/// A drawn pickup: its glint, and seconds until it twinkles (below zero: into the twinkle).
#[derive(Component)]
struct Drawn {
    glint: Option<Entity>,
    twinkle: f32,
}

#[derive(Component)]
struct Counter;

#[derive(Component)]
struct CounterTotal;

#[derive(Component)]
struct CounterGain;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup).add_systems(FixedUpdate, step.after(simulate)).add_systems(Update, (draw, counter));
}

fn model_file(kind: Kind) -> String {
    format!("bolt_{}_0{}.glb", kind.tier.name(), kind.variant + 1)
}

fn setup(
    mut commands: Commands,
    settings: Res<Settings>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let has = |f: &str| settings.model_dir.as_ref().is_some_and(|d| d.join(f).exists());
    let kinds: Vec<Kind> = Tier::ALL.into_iter().flat_map(|tier| (0..3).map(move |variant| Kind { tier, variant })).collect();
    let models = kinds.iter().all(|&k| has(&model_file(k))).then(|| kinds.iter().map(|&k| assets.load(GltfAssetLabel::Scene(0).from_asset(model_file(k)))).collect());
    match &models {
        Some(_) => info!("bolts: the game's pickup models (bolt_<tier>_0<n>.glb)"),
        None => info!("bolts: no riftapart-assets/bolt_<tier>_0<n>.glb (see tools/extract_riftapart_ratchet.py --character bolts); rings instead"),
    }
    // The stand-ins: rings about the size of the game's bolts, in the tiers' metal colours.
    let ring = meshes.add(Torus::new(0.05, 0.1));
    let colours = [(0.88, 0.63, 0.46), (0.6, 0.6, 0.62), (1.0, 0.89, 0.6), (0.95, 0.97, 0.91)];
    let rings = colours.iter().map(|&(r, g, b)| materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), metallic: 0.9, perceptual_roughness: 0.3, ..default() })).collect();
    let glint = has("vfx/bolt_glint.png").then(|| {
        // White (the game's glint colour rows are all 1), bright enough to bloom (ours).
        let material = StandardMaterial {
            base_color: LinearRgba::rgb(3.0, 3.0, 3.0).into(),
            base_color_texture: Some(assets.load("vfx/bolt_glint.png")),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            cull_mode: None,
            ..default()
        };
        (meshes.add(Rectangle::new(1.0, 1.0)), materials.add(material))
    });
    let world = BoltWorld::new(BoltParams::default().scaled(settings.scale), SEED);
    commands.insert_resource(Bolts {
        world,
        tally: Tally::new(0),
        dropped: 0,
        scale: settings.scale,
        drawn: HashMap::new(),
        models,
        ring: (ring, rings),
        glint,
        rng: Rng::new(SEED),
        flying: false,
    });
    // The counter: game UI, so up even with the debug overlay hidden.
    let shadow = TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) };
    commands
        .spawn((
            Counter,
            Node { position_type: PositionType::Absolute, top: px(16.0), right: px(28.0), flex_direction: FlexDirection::Column, align_items: AlignItems::End, ..default() },
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((CounterTotal, Text::new(""), TextFont::from_font_size(30.0), TextColor(Color::srgb(1.0, 0.95, 0.85)), shadow));
            c.spawn((CounterGain, Text::new(""), TextFont::from_font_size(24.0), TextColor(Color::srgb(1.0, 0.82, 0.35)), shadow));
        });
}

/// Drops the bolts of the dummies just knocked down, and moves every bolt one tick around
/// Ratchet (feet, body centre and thrown wrench) through the level's blocks.
#[allow(clippy::too_many_arguments)]
fn step(
    time: Res<Time>,
    settings: Res<Settings>,
    level: Res<Level>,
    wrench: Res<crate::wrench::Wrench>,
    players: Query<&Player>,
    mut dummies: Query<&mut Dummy>,
    mut bolts: ResMut<Bolts>,
    mut cues: MessageWriter<SoundCue>,
) {
    let Ok(p) = players.single() else { return };
    let (s, dt, now) = (settings.scale, time.delta_secs(), time.elapsed_secs());
    let b = &mut *bolts;
    if (b.scale - s).abs() > 1e-6 {
        b.scale = s;
        b.world.set_params(BoltParams::default().scaled(s));
    }
    for mut d in &mut dummies {
        if d.knocked_down {
            d.knocked_down = false;
            let n = b.world.drop_reward(d.pos + Vec3::Y * DROP_HEIGHT, DUMMY_VALUE, DEFAULT_LEVEL_MULTIPLIER);
            b.dropped += n;
            info!("bolts: a dummy went down at {now:.2} s: {n} bolts burst out");
        }
    }
    let hero = Hero { position: p.feet, center: p.feet + Vec3::Y * crate::BODY_HEIGHT * 0.5 * s, wrench: wrench.flight.as_ref().map(|f| f.position()) };
    b.world.step(dt, &hero, &|from, to| raycast(&level, from, to).map(|(point, normal, _)| Hit { point, normal }));
    for c in b.world.collected() {
        b.tally.add(c.bolts());
        cues.write(SoundCue::play("bolt_collect"));
    }
    b.tally.step(dt);
    let flying = b.world.pickups().iter().any(|p| p.state() == State::Flying);
    if b.flying && !flying {
        info!("bolts: all landed at {now:.2} s ({} lying)", b.world.pickups().iter().filter(|p| p.state() == State::Resting).count());
    }
    b.flying = flying;
}

/// Each pickup drawn where the sim has it (spawned when it appears, gone when it's collected),
/// turned and bobbing, with its twinkle.
fn draw(
    time: Res<Time>,
    mut commands: Commands,
    mut bolts: ResMut<Bolts>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut drawn: Query<(&mut Transform, &mut Drawn)>,
    mut glints: Query<&mut Transform, Without<Drawn>>,
) {
    let b = &mut *bolts;
    let (dt, s) = (time.delta_secs(), b.scale);
    let cam = cams.single().map_or((Vec3::ZERO, Quat::IDENTITY), |c| (c.translation(), c.rotation()));
    for p in b.world.pickups() {
        let tf = Transform { translation: p.position(), rotation: p.rotation(), scale: Vec3::splat(s) };
        let Some(&e) = b.drawn.get(&p.id()) else {
            let glint = b.glint.as_ref().map(|(mesh, mat)| commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::ZERO), NotShadowCaster)).id());
            let mut e = commands.spawn((tf, Visibility::default(), Drawn { glint, twinkle: b.rng.range(0.0, GLINT_EVERY.1) }));
            let (tier, look) = (p.kind().tier as usize, p.kind().variant as usize);
            match &b.models {
                Some(models) => {
                    e.with_child((WorldAssetRoot(models[tier * 3 + look].clone()), Transform::default()));
                }
                None => {
                    e.insert((Mesh3d(b.ring.0.clone()), MeshMaterial3d(b.ring.1[tier].clone())));
                }
            }
            if let Some(g) = glint {
                e.add_child(g);
            }
            b.drawn.insert(p.id(), e.id());
            continue;
        };
        let Ok((mut t, mut d)) = drawn.get_mut(e) else { continue };
        *t = tf;
        d.twinkle -= dt;
        if d.twinkle < -GLINT_LIFE {
            d.twinkle = b.rng.range(GLINT_EVERY.0, GLINT_EVERY.1);
        }
        if let Some(Ok(mut g)) = d.glint.map(|g| glints.get_mut(g)) {
            // Facing the camera, a little toward it, from inside the bolt's own turn and scale.
            let size = if d.twinkle < 0.0 { glint_size(-d.twinkle / GLINT_LIFE) } else { 0.0 };
            let back = tf.rotation.inverse();
            *g = Transform::from_translation(back * (cam.0 - tf.translation).normalize_or_zero() * GLINT_OUT).with_rotation(back * cam.1).with_scale(Vec3::splat(size * GLINT_SIZE));
        }
    }
    let live: HashSet<u32> = b.world.pickups().iter().map(|p| p.id()).collect();
    b.drawn.retain(|id, e| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
}

/// The glint's size `t` (0..1) into its twinkle.
fn glint_size(t: f32) -> f32 {
    let i = GLINT_CURVE.iter().position(|&(at, _)| at > t).unwrap_or(GLINT_CURVE.len() - 1).max(1);
    let ((t0, v0), (t1, v1)) = (GLINT_CURVE[i - 1], GLINT_CURVE[i]);
    v0 + (v1 - v0) * ((t - t0) / (t1 - t0)).clamp(0.0, 1.0)
}

/// The counter: the total it shows, the "+N" waiting beside it, up while the tally says so.
fn counter(
    bolts: Res<Bolts>,
    mut counter: Query<&mut Visibility, With<Counter>>,
    mut total: Query<&mut Text, (With<CounterTotal>, Without<CounterGain>)>,
    mut gain: Query<&mut Text, (With<CounterGain>, Without<CounterTotal>)>,
) {
    let t = &bolts.tally;
    if let Ok(mut v) = counter.single_mut() {
        v.set_if_neq(if t.visible() { Visibility::Inherited } else { Visibility::Hidden });
    }
    if let Ok(mut text) = total.single_mut() {
        text.set_if_neq(Text::new(format!("BOLTS  {}", thousands(t.shown()))));
    }
    if let Ok(mut text) = gain.single_mut() {
        text.set_if_neq(Text::new(if t.gain() > 0 { format!("+{}", thousands(t.gain())) } else { String::new() }));
    }
}

/// 1234567 -> "1,234,567".
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The debug HUD's line.
pub fn hud_line(b: &Bolts) -> String {
    let count = |s: State| b.world.pickups().iter().filter(|p| p.state() == s).count();
    format!(
        "bolts: {} dropped, {} collected  |  pickups lying {}, flying {}, waiting {}, coming {}",
        b.dropped,
        b.tally.total(),
        count(State::Resting),
        count(State::Flying),
        count(State::Queued),
        count(State::Pulled)
    )
}

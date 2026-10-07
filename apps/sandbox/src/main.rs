//! libratchet sandbox: a small course to feel and measure the movement.
//!
//! Run: `cargo run -p sandbox`
//! Options:
//!   --configs <dir>   folder with hero_traversal.json / hero_shared.json extracted from your own
//!                     Rift Apart install by tools/extract_riftapart_configs.py (default:
//!                     $RIFTAPART_CONFIGS, else ./riftapart-configs, else ../riftapart-decomp/configs)
//!   --scale <s>       start at this character scale (1 = Ratchet-sized)
//!   --demo            play a scripted run, save screenshots to ./screenshots, then quit
//!   --record <dir>    with --demo: also save every frame (60 fps) as PNG for a video
//!   --demo-part wrench  with --demo: only the wrench section

mod clank;
mod combat;
mod demo;
mod level;
mod ratchet_model;
mod wrench;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowResolution};
use level::{Level, spots};
use libratchet::{Contact, Event as MoveEvent, Input as MoveInput, MoveState, Ratchet, Tuning};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

const SIM_HZ: f64 = 120.0;
/// Ratchet-sized collision box (scale 1).
const BODY_HEIGHT: f32 = 1.2;
const BODY_HALF_WIDTH: f32 = 0.35;
const STEP_HEIGHT: f32 = 0.35;
/// Roughly how much bigger the Dragonborn is than Ratchet.
const DRAGONBORN_SCALE: f32 = 1.55;
const TRAIL_LEN: usize = 4000;

#[derive(Resource)]
pub struct Settings {
    base: Tuning,
    scale: f32,
    configs_dir: PathBuf,
    load_note: String,
    /// `riftapart-assets/` with the converted Ratchet model, when present.
    model_dir: Option<PathBuf>,
}

#[derive(Component)]
pub struct Player {
    sim: Ratchet,
    feet: Vec3,
    contact: Contact,
    stats: JumpStats,
}

#[derive(Default, Clone, Copy)]
struct JumpStats {
    airborne: bool,
    takeoff: Vec3,
    peak: Vec3,
    airtime: f32,
    last: Option<LastJump>,
}

#[derive(Clone, Copy)]
struct LastJump {
    peak_above_takeoff: f32,
    airtime: f32,
    distance: f32,
}

#[derive(Component)]
struct Body;

#[derive(Component)]
struct HudText;

#[derive(Resource)]
pub struct CameraRig {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
    /// When set, the camera looks at this fixed spot instead of following (demo wide shots).
    fixed: Option<(Vec3, f32, f32, f32)>,
    /// Seconds of camera shake left (Hyper-Strike landings).
    pub shake: f32,
}

#[derive(Resource, Default)]
pub struct Trail {
    points: VecDeque<(Vec3, MoveState)>,
    apexes: Vec<(Vec3, f32)>,
}

/// Input from the demo script; overrides the keyboard when set.
#[derive(Resource, Default)]
pub struct ScriptedInput(pub Option<MoveInput>);

/// Left mouse is melee only for presses that start with the mouse already grabbed (the click
/// that grabs it doesn't swing).
#[derive(Resource, Default)]
pub struct MouseMelee(bool);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let demo = args.iter().any(|a| a == "--demo");
    let record = arg("--record").map(PathBuf::from);
    let scale = arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let configs_dir = arg("--configs")
        .or_else(|| std::env::var("RIFTAPART_CONFIGS").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            ["riftapart-configs", "../riftapart-decomp/configs"]
                .into_iter()
                .map(PathBuf::from)
                .find(|d| d.join("hero_traversal.json").exists())
                .unwrap_or_else(|| PathBuf::from("riftapart-configs"))
        });
    let (base, load_note) = load_tuning(&configs_dir);

    let resolution = if demo { (1280, 720) } else { (1600, 900) };
    let model_dir = ratchet_model::find_assets_dir();
    match &model_dir {
        Some(d) => info!("Ratchet model: {}", d.join(ratchet_model::MODEL_FILE).display()),
        None => info!("no riftapart-assets/{} found; using the capsule (see tools/extract_riftapart_ratchet.py)", ratchet_model::MODEL_FILE),
    }
    let asset_root = model_dir.as_ref().map_or_else(|| "assets".to_string(), |d| d.to_string_lossy().into_owned());
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "libratchet sandbox".into(),
                    resolution: WindowResolution::from(resolution),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin { file_path: asset_root, ..default() }),
    )
    .add_plugins((ratchet_model::plugin, clank::plugin, combat::plugin, wrench::plugin))
    .insert_resource(Time::<Fixed>::from_hz(SIM_HZ))
    .insert_resource(ClearColor(Color::srgb(0.55, 0.7, 0.9)))
    .insert_resource(Settings { base, scale, configs_dir, load_note, model_dir })
    .insert_resource(CameraRig { yaw: 0.6, pitch: 0.35, distance: 7.0, focus: spots::SPAWN, fixed: None, shake: 0.0 })
    .init_resource::<Trail>()
    .init_resource::<ScriptedInput>()
    .init_resource::<MouseMelee>()
    .add_systems(Startup, (level::spawn_level, setup).chain())
    .add_systems(FixedUpdate, simulate)
    .add_systems(Update, (hotkeys, cursor_grab, camera_control, update_visuals, camera_follow, draw_trail, update_hud).chain());

    if demo {
        let out = PathBuf::from("screenshots");
        std::fs::create_dir_all(&out).ok();
        if let Some(dir) = &record {
            std::fs::create_dir_all(dir).ok();
            // Every frame advances exactly 1/60 s, so saved frames play back at real speed.
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
        }
        demo::add(&mut app, out, record, arg("--demo-part").as_deref());
    }
    app.run();
}

fn load_tuning(dir: &std::path::Path) -> (Tuning, String) {
    match Tuning::from_riftapart_configs(dir) {
        Ok(t) => (t, "Rift Apart configs from your install".into()),
        Err(e) => {
            let note = format!("generic defaults (couldn't load Rift Apart configs: {e})");
            warn!("{note}");
            (Tuning::default(), note)
        }
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    settings: Res<Settings>,
    asset_server: Res<AssetServer>,
) {
    // A sky-to-ground gradient as environment light, so metal (Clank!) has something to reflect.
    let mut sky = EnvironmentMapLight::hemispherical_gradient(
        &mut images,
        Color::srgb(0.55, 0.7, 0.95),
        Color::srgb(0.85, 0.85, 0.85),
        Color::srgb(0.35, 0.33, 0.3),
    );
    sky.intensity = 1500.0;
    commands.spawn((Camera3d::default(), sky, Transform::from_xyz(-6.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 250.0, ..default() });

    let tuning = settings.base.scaled(settings.scale);
    let fur = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.55, 0.15), ..default() });
    let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.15, 0.12, 0.2), ..default() });
    let r = BODY_HALF_WIDTH;
    let player = commands
        .spawn((
            Player { sim: Ratchet::new(tuning), feet: spots::SPAWN, contact: Contact::default(), stats: JumpStats::default() },
            Transform::from_translation(spots::SPAWN),
            Visibility::default(),
        ))
        .with_children(|p| {
            // Body: a capsule standing on the feet point.
            p.spawn((
                Body,
                ratchet_model::Placeholder,
                Mesh3d(meshes.add(Capsule3d::new(r, BODY_HEIGHT - 2.0 * r))),
                MeshMaterial3d(fur.clone()),
                Transform::from_xyz(0.0, BODY_HEIGHT * 0.5, 0.0),
            ));
            // Visor shows which way he faces (+Z in local space).
            p.spawn((
                ratchet_model::Placeholder,
                Mesh3d(meshes.add(Cuboid::new(0.4, 0.14, 0.12))),
                MeshMaterial3d(dark.clone()),
                Transform::from_xyz(0.0, BODY_HEIGHT * 0.78, r * 0.9),
            ));
            // Lombax ears, because why not.
            for side in [-1.0f32, 1.0] {
                p.spawn((
                    ratchet_model::Placeholder,
                    Mesh3d(meshes.add(Cone { radius: 0.09, height: 0.45 })),
                    MeshMaterial3d(fur.clone()),
                    Transform::from_xyz(side * 0.2, BODY_HEIGHT + 0.12, 0.0).with_rotation(Quat::from_rotation_z(-side * 0.5)),
                ));
            }
        })
        .id();
    if settings.model_dir.is_some() {
        ratchet_model::spawn(&mut commands, &asset_server, player);
    }

    commands.spawn((
        HudText,
        Text::new(""),
        TextFont::from_font_size(15.0),
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, top: px(10.0), left: px(12.0), ..default() },
        TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
    ));
    commands.spawn((
        Text::new(
            "WASD move  |  Space jump (tap/hold = higher, again in the air = double jump, hold = glide)  |  Shift dash (also in the air)  |  Ctrl walk\n\
             Left mouse / E wrench (in the air: Hyper-Strike)  |  Right mouse / Q throw the wrench  |  Tab Ratchet/Dragonborn size  |  R reload tuning\n\
             T clear trail  |  Backspace respawn  |  click to grab the mouse, Esc to free it",
        ),
        TextFont::from_font_size(13.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node { position_type: PositionType::Absolute, bottom: px(10.0), left: px(12.0), ..default() },
        TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
    ));
}

fn keyboard_input(keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>, mouse_melee: bool, yaw: f32) -> MoveInput {
    let mut forward = 0.0;
    let mut side = 0.0;
    if keys.pressed(KeyCode::KeyW) {
        forward += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        forward -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        side += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        side -= 1.0;
    }
    // The camera sits at +yaw around the player and looks back at him.
    let fwd = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(-fwd.y, fwd.x);
    let dir = (fwd * forward + right * side).normalize_or_zero();
    let walk = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    MoveInput {
        move_dir: dir * if walk { 0.45 } else { 1.0 },
        jump: keys.pressed(KeyCode::Space),
        dash: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        melee: keys.pressed(KeyCode::KeyE) || (mouse_melee && mouse.pressed(MouseButton::Left)),
    }
}

pub fn simulate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_melee: Res<MouseMelee>,
    rig: Res<CameraRig>,
    scripted: Res<ScriptedInput>,
    level: Res<Level>,
    settings: Res<Settings>,
    mut players: Query<&mut Player>,
    mut trail: ResMut<Trail>,
) {
    let dt = time.delta_secs();
    let Ok(mut p) = players.single_mut() else { return };
    let s = settings.scale;
    let (half_w, height) = (BODY_HALF_WIDTH * s, BODY_HEIGHT * s);
    let input = scripted.0.unwrap_or_else(|| keyboard_input(&keys, &mouse, mouse_melee.0, rig.yaw));

    let before = p.feet;
    let contact = p.contact;
    let v = p.sim.step(dt, &input, &contact, before);
    let moved = level::move_box(&level, before, half_w, height, v * dt, STEP_HEIGHT * s);

    // Walls stop the sim's horizontal speed too, so it doesn't keep pushing.
    if moved.hit_wall {
        let actual = (moved.feet - before) / dt;
        let mut vel = p.sim.velocity();
        if (actual.x - v.x).abs() > 1e-3 {
            vel.x = actual.x;
        }
        if (actual.z - v.z).abs() > 1e-3 {
            vel.z = actual.z;
        }
        p.sim.set_velocity(vel);
    }
    let mut feet = moved.feet;
    let mut grounded = moved.grounded;
    let mut ground_distance = level::ground_distance(&level, feet, half_w);
    // Walking down steps: stay glued to the ground instead of falling for a few ticks.
    if !grounded && p.sim.state() == MoveState::Ground && v.y <= 0.0 {
        if let Some(d) = ground_distance.filter(|d| *d <= STEP_HEIGHT * s) {
            feet.y -= d;
            grounded = true;
            ground_distance = Some(0.0);
        }
    }
    p.feet = feet;
    p.contact = Contact { grounded, ground_distance, hit_ceiling: moved.hit_ceiling };

    // Jump measurements for the HUD.
    let jumped = p.sim.events().iter().any(|e| matches!(e, MoveEvent::Jumped));
    let stats = &mut p.stats;
    if jumped || (!stats.airborne && !grounded) {
        *stats = JumpStats { airborne: true, takeoff: before, peak: feet, airtime: 0.0, last: stats.last };
    } else if stats.airborne {
        stats.airtime += dt;
        if feet.y > stats.peak.y {
            stats.peak = feet;
        }
        if grounded {
            stats.airborne = false;
            let height = stats.peak.y - stats.takeoff.y;
            let distance = Vec2::new(feet.x - stats.takeoff.x, feet.z - stats.takeoff.z).length();
            stats.last = Some(LastJump { peak_above_takeoff: height, airtime: stats.airtime, distance });
            if height > 0.05 {
                trail.apexes.push((stats.peak, height));
            }
        }
    }

    trail.points.push_back((feet, p.sim.state()));
    while trail.points.len() > TRAIL_LEN {
        trail.points.pop_front();
    }

    if feet.y < -30.0 {
        respawn(&mut p, spots::SPAWN);
    }
}

pub fn respawn(p: &mut Player, at: Vec3) {
    let tuning = p.sim.tuning().clone();
    p.sim = Ratchet::new(tuning);
    p.feet = at;
    p.contact = Contact::default();
    p.stats = JumpStats::default();
}

fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<Settings>,
    mut players: Query<&mut Player>,
    mut trail: ResMut<Trail>,
) {
    let Ok(mut p) = players.single_mut() else { return };
    if keys.just_pressed(KeyCode::Tab) {
        settings.scale = if (settings.scale - 1.0).abs() < 0.01 { DRAGONBORN_SCALE } else { 1.0 };
        let t = settings.base.scaled(settings.scale);
        p.sim.set_tuning(t);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let (base, note) = load_tuning(&settings.configs_dir);
        settings.base = base;
        settings.load_note = note;
        let t = settings.base.scaled(settings.scale);
        p.sim.set_tuning(t);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        *trail = Trail::default();
    }
    if keys.just_pressed(KeyCode::Backspace) {
        respawn(&mut p, spots::SPAWN);
    }
}

fn cursor_grab(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    scripted: Res<ScriptedInput>,
    mut mouse_melee: ResMut<MouseMelee>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut c) = cursor.single_mut() else { return };
    if scripted.0.is_some() {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        mouse_melee.0 = c.grab_mode != CursorGrabMode::None;
        c.grab_mode = CursorGrabMode::Locked;
        c.visible = false;
    }
    if mouse.just_released(MouseButton::Left) {
        mouse_melee.0 = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        c.grab_mode = CursorGrabMode::None;
        c.visible = true;
    }
}

fn camera_control(
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    mut rig: ResMut<CameraRig>,
) {
    let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    if grabbed {
        rig.yaw -= motion.delta.x * 0.003;
        rig.pitch = (rig.pitch + motion.delta.y * 0.003).clamp(-0.25, 1.35);
    }
    rig.distance = (rig.distance - scroll.delta.y * 0.6).clamp(2.0, 40.0);
}

fn state_color(state: MoveState) -> Color {
    match state {
        MoveState::Ground => Color::srgb(0.95, 0.55, 0.15),
        MoveState::Jump => Color::srgb(1.0, 0.9, 0.2),
        MoveState::DoubleJump => Color::srgb(0.95, 0.3, 0.9),
        MoveState::Fall => Color::srgb(0.95, 0.2, 0.2),
        MoveState::Glide => Color::srgb(0.2, 0.9, 1.0),
        MoveState::Dash => Color::srgb(0.55, 0.35, 1.0),
        MoveState::AirDash => Color::srgb(0.2, 1.0, 0.45),
        MoveState::Melee => Color::srgb(0.95, 0.95, 0.95),
        MoveState::Smash | MoveState::SmashLand => Color::srgb(1.0, 0.45, 0.6),
    }
}

fn update_visuals(
    settings: Res<Settings>,
    mut players: Query<(&Player, &mut Transform)>,
    bodies: Query<&MeshMaterial3d<StandardMaterial>, With<Body>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((p, mut tf)) = players.single_mut() else { return };
    tf.translation = p.feet;
    tf.rotation = Quat::from_rotation_y(p.sim.facing_yaw());
    tf.scale = Vec3::splat(settings.scale);
    if let Ok(handle) = bodies.single()
        && let Some(mut m) = materials.get_mut(&handle.0)
    {
        m.base_color = state_color(p.sim.state());
    }
}

fn camera_follow(
    time: Res<Time>,
    settings: Res<Settings>,
    mut rig: ResMut<CameraRig>,
    players: Query<&Player>,
    mut cams: Query<&mut Transform, With<Camera3d>>,
) {
    let (Ok(p), Ok(mut cam)) = (players.single(), cams.single_mut()) else { return };
    if let Some((focus, yaw, pitch, distance)) = rig.fixed {
        let offset = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance;
        *cam = Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y);
        rig.focus = focus;
        return;
    }
    let target = p.feet + Vec3::Y * 0.9 * settings.scale;
    let k = 1.0 - (-10.0 * time.delta_secs()).exp();
    rig.focus = rig.focus.lerp(target, k);
    // A short 20 Hz wobble, fading out (like the game's melee-impact shake).
    rig.shake = (rig.shake - time.delta_secs()).max(0.0);
    let wobble = (time.elapsed_secs() * 20.0 * std::f32::consts::TAU).sin() * (rig.shake / 0.25).min(1.0);
    let (yaw, pitch) = (rig.yaw + wobble * 0.012, rig.pitch + wobble * 0.004);
    let offset = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * rig.distance;
    *cam = Transform::from_translation(rig.focus + offset).looking_at(rig.focus, Vec3::Y);
}

fn draw_trail(trail: Res<Trail>, mut gizmos: Gizmos) {
    let lift = Vec3::Y * 0.05;
    for pair in trail.points.as_slices().0.windows(2).chain(trail.points.as_slices().1.windows(2)) {
        let ((a, _), (b, state)) = (pair[0], pair[1]);
        if a.distance(b) < 2.0 {
            gizmos.line(a + lift, b + lift, state_color(state));
        }
    }
    for (peak, height) in &trail.apexes {
        let base = *peak - Vec3::Y * *height;
        gizmos.line(base, *peak, Color::srgba(1.0, 1.0, 1.0, 0.35));
        gizmos.cross(Isometry3d::from_translation(*peak), 0.15, Color::WHITE);
    }
}

fn update_hud(settings: Res<Settings>, hits: Res<combat::Hits>, players: Query<&Player>, mut hud: Query<&mut Text, With<HudText>>) {
    let (Ok(p), Ok(mut text)) = (players.single(), hud.single_mut()) else { return };
    let v = p.sim.velocity();
    let size = if (settings.scale - 1.0).abs() < 0.01 { "Ratchet-sized".to_string() } else { format!("{:.2}x, Dragonborn-sized", settings.scale) };
    let ground = p.contact.ground_distance.map_or("-".into(), |d| format!("{d:.2} m"));
    let last = p.stats.last.map_or("-".into(), |j| {
        format!("peak {:.2} m above take-off  |  air time {:.2} s  |  distance {:.2} m", j.peak_above_takeoff, j.airtime, j.distance)
    });
    let t = p.sim.tuning();
    text.0 = format!(
        "tuning: {} ({size})\n\
         state: {:<12} speed {:.2} m/s   vertical {:+.2} m/s   above ground {ground}\n\
         last jump: {last}\n\
         config: jump tap {:.2} m in {:.2} s, held up to {:.2} m  |  double +{:.2} m (max total {})  |  glide falls {:.1} m/s, moves {:.2} m/s  |  run {:.2} m/s\n\
         dash {:.1} m up to {:.1} m/s  |  air dash {:.1} m at {:.1} m/s\n\
         wrench: swings {}  |  Hyper-Strike hop {:.2} m, slam at {:.0} m/s², {:.1} m wide  |  throw {:.0} m/s out to {:.0} m
         dummies hit: {}  {}",
        settings.load_note,
        p.sim.state().name(),
        Vec2::new(v.x, v.z).length(),
        if p.sim.state() == MoveState::Ground { 0.0 } else { v.y },
        t.single_jump.height_min,
        t.single_jump.time_to_peak,
        t.single_jump.height_max,
        t.double_jump.height_max,
        t.double_jump.max_total_jump.map_or("-".into(), |m| format!("{m:.2} m")),
        t.glide.terminal_velocity,
        t.glide_speeds.run_max,
        t.ground_speeds.run_max,
        t.ground_dash.distance,
        t.ground_dash.max_speed,
        t.air_dash.distance,
        t.air_dash.max_speed,
        t.melee
            .combo
            .iter()
            .map(|w| format!("{:.2} s (hit {:.2} s, lunge {:.2} m)", w.duration, w.hit_time, w.lunge.last().copied().unwrap_or(0.0)))
            .collect::<Vec<_>>()
            .join(", "),
        t.melee.smash_hop.height_min,
        t.melee.smash_gravity,
        t.melee.smash_radius,
        t.melee.throw.out_speed,
        t.melee.throw.max_distance,
        hits.count,
        if hits.last.is_empty() { String::new() } else { format!("(last: {})", hits.last) },
    );
}

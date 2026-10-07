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
//!   --demo-part wrench|weapons|moves|combat|face|sounds  with --demo: only that section
//!   --clean           start with the debug overlay (HUD, help, trails) hidden; F1 toggles it

mod camera;
mod clank;
mod combat;
mod demo;
mod dummy_model;
mod level;
mod ratchet_model;
mod vfx;
mod sound;
mod weapons;
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

/// Demo runs draw into this image instead of the window, so their screenshots and recordings
/// work whatever happens to the window (minimised, behind a game).
#[derive(Resource, Default)]
pub struct DemoView(pub Option<Handle<Image>>);

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
    /// Seconds of camera wobble left (Hyper-Strike landings; ours).
    pub shake: f32,
    /// 0..1: how far the camera has moved over the shoulder for aiming, and back for running.
    aim_blend: f32,
    moving: f32,
    /// The game's shakes playing: shake, seconds in, strength.
    shakes: Vec<(camera::Shake, f32, f32)>,
    /// Demo close-ups: look at his chest.
    close_up: bool,
}

impl CameraRig {
    /// Starts one of the game's camera shakes (`strength` 0..1).
    pub fn add_shake(&mut self, shake: &camera::Shake, strength: f32) {
        if strength > 0.0 {
            self.shakes.push((shake.clone(), 0.0, strength));
        }
    }
}

#[derive(Resource, Default)]
pub struct Trail {
    points: VecDeque<(Vec3, MoveState)>,
    apexes: Vec<(Vec3, f32)>,
}

/// Input from the demo script; overrides the keyboard when set.
#[derive(Resource, Default)]
pub struct ScriptedInput(pub Option<MoveInput>);

/// Left mouse fires only for presses that start with the mouse already grabbed (the click that
/// grabs it doesn't shoot).
#[derive(Resource, Default)]
pub struct MouseMelee(bool);

impl MouseMelee {
    /// The left button is down from a press that started with the mouse grabbed.
    pub fn armed(&self) -> bool {
        self.0
    }
}

#[derive(Component)]
struct Crosshair;

/// Debug text and drawings (HUD, help, trails), hidden with F1 for recording videos.
#[derive(Component)]
struct DebugUi;

#[derive(Resource)]
struct DebugOverlay(bool);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let demo = args.iter().any(|a| a == "--demo");
    let record = arg("--record").map(PathBuf::from);
    let clean = args.iter().any(|a| a == "--clean");
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
    let camera_configs = camera::CameraConfigs::load(&configs_dir);

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
                    // A demo runs offscreen (DemoView); its window shouldn't take focus.
                    focused: !demo,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin { file_path: asset_root, ..default() }),
    )
    .add_plugins((ratchet_model::plugin, clank::plugin, combat::plugin, dummy_model::plugin, wrench::plugin, weapons::plugin, vfx::plugin, sound::plugin))
    .insert_resource(Time::<Fixed>::from_hz(SIM_HZ))
    // The sky: with HDR the clear colour goes through the tonemapper too, so this is the colour
    // that comes out as sRGB (0.55, 0.7, 0.9) after it.
    .insert_resource(ClearColor(Color::linear_rgb(0.34, 0.73, 1.75)))
    .insert_resource(Settings { base, scale, configs_dir, load_note, model_dir })
    .insert_resource(DemoView(demo.then(Handle::default)))
    .insert_resource(CameraRig {
        yaw: 0.6,
        pitch: 0.35,
        distance: camera_configs.stand.level.z * scale,
        focus: spots::SPAWN,
        fixed: None,
        shake: 0.0,
        aim_blend: 0.0,
        moving: 0.0,
        shakes: Vec::new(),
        close_up: false,
    })
    .insert_resource(camera_configs)
    .insert_resource(DebugOverlay(!clean))
    .init_resource::<Trail>()
    .init_resource::<ScriptedInput>()
    .init_resource::<MouseMelee>()
    .add_systems(Startup, ((level::spawn_level, setup).chain(), camera::log_source))
    .add_systems(FixedUpdate, simulate)
    .add_systems(Update, (hotkeys, cursor_grab, camera_control, update_visuals, camera_follow, draw_trail, update_hud, crosshair, debug_overlay).chain());

    if demo {
        let out = PathBuf::from("screenshots");
        std::fs::create_dir_all(&out).ok();
        if let Some(dir) = &record {
            std::fs::create_dir_all(dir).ok();
        }
        // Every frame advances exactly 1/60 s: saved frames play back at real speed, and a
        // screenshot shows the moment it was asked for even when frames come slowly (another
        // game on the GPU, a minimised window).
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
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
    mut view: ResMut<DemoView>,
) {
    // A sky-to-ground gradient as environment light, so metal (Clank!) has something to reflect.
    let mut sky = EnvironmentMapLight::hemispherical_gradient(
        &mut images,
        Color::srgb(0.55, 0.7, 0.95),
        Color::srgb(0.85, 0.85, 0.85),
        Color::srgb(0.35, 0.33, 0.3),
    );
    sky.intensity = 1500.0;
    // HDR with a light bloom, so bright effects (muzzle flashes, tracers, explosions) glow the
    // way they do in the game.
    let mut camera = commands.spawn((
        Camera3d::default(),
        bevy::camera::Hdr,
        bevy::post_process::bloom::Bloom { intensity: 0.1, ..bevy::post_process::bloom::Bloom::NATURAL },
        sky,
        Transform::from_xyz(-6.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if view.0.is_some() {
        let format = bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb;
        let target = images.add(Image::new_target_texture(1280, 720, format, None));
        camera.insert((bevy::camera::RenderTarget::Image(target.clone().into()), IsDefaultUiCamera));
        view.0 = Some(target);
    }
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
        DebugUi,
        Text::new(""),
        TextFont::from_font_size(15.0),
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, top: px(10.0), left: px(12.0), ..default() },
        TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
    ));
    commands.spawn((
        Text::new(
            "WASD move  |  Space jump (tap/hold = higher, again in the air = double jump, hold = glide)  |  Shift dash (also in the air)  |  Ctrl walk\n\
             1 Burst Pistol, 2 Shatterbomb  |  left mouse fire  |  F melee (the wrench), in the air: Hyper-Strike\n\
             right mouse aim; melee while aiming = throw the wrench (Q: one-button throw)  |  Tab Ratchet/Dragonborn size  |  R reload tuning  |  T clear trail  |  Backspace respawn  |  click to grab the mouse, Esc to free it  |  F1 hide/show this",
        ),
        DebugUi,
        TextFont::from_font_size(13.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node { position_type: PositionType::Absolute, bottom: px(10.0), left: px(12.0), ..default() },
        TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
    ));
    // Crosshair in the middle of the screen, shown while a gun is out.
    commands.spawn((
        Crosshair,
        Text::new("+"),
        TextFont::from_font_size(28.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
        TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
        Node { position_type: PositionType::Absolute, left: percent(50.0), top: percent(50.0), margin: UiRect::new(px(-8.0), px(0.0), px(-17.0), px(0.0)), ..default() },
        Visibility::Hidden,
    ));
}

/// F1 hides or shows the debug text (the trails check it when they draw).
fn debug_overlay(overlay: Res<DebugOverlay>, mut q: Query<&mut Visibility, With<DebugUi>>) {
    if overlay.is_changed() {
        for mut v in &mut q {
            v.set_if_neq(if overlay.0 { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
}

fn crosshair(arsenal: Option<Res<weapons::Arsenal>>, mut q: Query<&mut Visibility, With<Crosshair>>) {
    let show = arsenal.is_some();
    for mut v in &mut q {
        v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}

fn keyboard_input(keys: &ButtonInput<KeyCode>, yaw: f32) -> MoveInput {
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
        // Melee is F, as in the game.
        melee: keys.pressed(KeyCode::KeyF),
        aim: None,
        aim_mode: false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn simulate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    arsenal: Option<Res<weapons::Arsenal>>,
    wrench: Option<Res<wrench::Wrench>>,
    rig: Res<CameraRig>,
    scripted: Res<ScriptedInput>,
    level: Res<Level>,
    settings: Res<Settings>,
    mut players: Query<&mut Player>,
    mut trail: ResMut<Trail>,
    (mut cues, mut was_gliding, mut melee_held): (MessageWriter<sound::SoundCue>, Local<bool>, Local<bool>),
) {
    let dt = time.delta_secs();
    let Ok(mut p) = players.single_mut() else { return };
    let s = settings.scale;
    let (half_w, height) = (BODY_HALF_WIDTH * s, BODY_HEIGHT * s);
    let mut input = scripted.0.unwrap_or_else(|| keyboard_input(&keys, rig.yaw));
    // Aiming, or firing a gun (and just after the newest shot): face where the camera looks and
    // strafe, as in the game. While aiming, melee throws the wrench instead of swinging
    // (wrench.rs).
    let aiming = arsenal.as_ref().is_some_and(|a| a.aiming);
    // (A gun put away for a wrench throw doesn't fire, so it doesn't strafe either.)
    let throwing = wrench.as_ref().is_some_and(|w| w.throwing());
    if aiming || arsenal.as_ref().is_some_and(|a| !throwing && (a.trigger || a.stance > 0.0)) {
        input.aim = Some(Vec2::new(-rig.yaw.sin(), -rig.yaw.cos()));
        input.aim_mode = aiming;
    }
    // Melee does nothing while the wrench is out of the hand (thrown, or leaving it), as in the
    // game, and a press held through the catch doesn't swing when it's back.
    let blocked = aiming || wrench.as_ref().is_some_and(|w| w.out_of_hand());
    if input.melee && (blocked || *melee_held) {
        *melee_held = true;
        input.melee = false;
    } else if !input.melee {
        *melee_held = false;
    }

    let before = p.feet;
    let contact = p.contact;
    let v = p.sim.step(dt, &input, &contact, before);
    // The game's sounds for what the moves do (the animation clips sound the rest: steps,
    // jumps, swings, efforts). The heli-pack unfolds, spins while gliding, and folds away.
    for e in p.sim.events() {
        match e {
            MoveEvent::Jumped => drop(cues.write(sound::SoundCue::play("footstep_jump"))),
            MoveEvent::Landed { .. } => {
                cues.write(sound::SoundCue::play("land"));
                cues.write(sound::SoundCue::play("footstep_land"));
            }
            MoveEvent::Dashed => drop(cues.write(sound::SoundCue::play("phantom_dash"))),
            MoveEvent::AirDashed => drop(cues.write(sound::SoundCue::play("air_dash"))),
            _ => {}
        }
    }
    let gliding = p.sim.state() == MoveState::Glide;
    if gliding != *was_gliding {
        *was_gliding = gliding;
        if gliding {
            cues.write(sound::SoundCue::play("heli_extend"));
            cues.write(sound::SoundCue::start("heli_loop"));
        } else {
            cues.write(sound::SoundCue::stop("heli_loop"));
            cues.write(sound::SoundCue::play("heli_retract"));
        }
    }
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
    mut rig: ResMut<CameraRig>,
    mut overlay: ResMut<DebugOverlay>,
) {
    if keys.just_pressed(KeyCode::F1) {
        overlay.0 = !overlay.0;
    }
    let Ok(mut p) = players.single_mut() else { return };
    if keys.just_pressed(KeyCode::Tab) {
        let old = settings.scale;
        settings.scale = if (settings.scale - 1.0).abs() < 0.01 { DRAGONBORN_SCALE } else { 1.0 };
        rig.distance *= settings.scale / old;
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

/// The follow camera, placed by the game's camera contexts (`camera.rs`): around a pivot above
/// Ratchet's feet, further back while he runs, over his right shoulder while aiming.
#[allow(clippy::too_many_arguments)]
fn camera_follow(
    time: Res<Time>,
    settings: Res<Settings>,
    configs: Res<camera::CameraConfigs>,
    arsenal: Option<Res<weapons::Arsenal>>,
    mut rig: ResMut<CameraRig>,
    players: Query<&Player>,
    mut cams: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    let (Ok(p), Ok((mut cam, mut projection))) = (players.single(), cams.single_mut()) else { return };
    let dt = time.delta_secs();
    // The game's shakes, added up (degrees of yaw, pitch and roll).
    let mut shake = Vec3::ZERO;
    rig.shakes.retain_mut(|(sh, age, strength)| {
        *age += dt;
        sh.sample(*age).map(|v| shake += v * *strength).is_some()
    });
    let shake = Quat::from_euler(EulerRot::YXZ, shake.x.to_radians(), shake.y.to_radians(), shake.z.to_radians());
    if let Some((focus, yaw, pitch, distance)) = rig.fixed {
        let offset = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance;
        *cam = Transform::from_translation(focus + offset).looking_at(focus, Vec3::Y);
        cam.rotation *= shake;
        set_fov(&mut projection, configs.stand.fov);
        return;
    }
    let s = settings.scale;
    let k = 1.0 - (-10.0 * dt).exp();
    rig.focus = rig.focus.lerp(p.feet, k);
    // A short 20 Hz wobble, fading out (ours, for melee impacts).
    rig.shake = (rig.shake - dt).max(0.0);
    let wobble = (time.elapsed_secs() * 20.0 * std::f32::consts::TAU).sin() * (rig.shake / 0.25).min(1.0);
    let (yaw, pitch) = (rig.yaw + wobble * 0.012, rig.pitch + wobble * 0.004);
    // Aiming blends to the strafe context (over about 0.13 s); running, to the traversal place.
    let aiming = arsenal.is_some_and(|a| a.aiming);
    let k = 1.0 - (-dt / 0.13).exp();
    rig.aim_blend += ((aiming as u8 as f32) - rig.aim_blend) * k;
    let speed = p.sim.velocity().with_y(0.0).length() / s;
    let k = 1.0 - (-dt / 0.5).exp();
    rig.moving += (camera::CameraContext::moving(speed) - rig.moving) * k;
    // The mouse wheel (and the demo) set how far back the standing camera is; zoomed in closer
    // than the game's distance, it looks lower, toward his chest (ours), and stops pulling back
    // while he runs.
    let zoom = rig.distance / (configs.stand.level.z * s).max(0.01);
    let game_framing = if rig.close_up { 0.0 } else { ((zoom - 0.45) / 0.55).clamp(0.0, 1.0) };
    let (stand_pivot, mut stand, stand_fov) = configs.stand.place(pitch, rig.moving * game_framing);
    let stand_pivot = CHEST + (stand_pivot - CHEST) * game_framing;
    stand.z *= zoom;
    let (aim_pivot, aim, aim_fov) = configs.aim.place(pitch, 0.0);
    let b = rig.aim_blend;
    let pivot = (stand_pivot + (aim_pivot - stand_pivot) * b) * s;
    let offset = stand.lerp(aim, b) * s;
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let focus = rig.focus + Vec3::Y * (pivot + offset.y) + right * offset.x;
    let back = Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos());
    *cam = Transform::from_translation(focus + back * offset.z).looking_at(focus, Vec3::Y);
    cam.rotation *= shake;
    set_fov(&mut projection, stand_fov + (aim_fov - stand_fov) * b);
}

/// Where a zoomed-in camera looks (height above his feet at Ratchet's size; ours).
const CHEST: f32 = 0.8;

/// Sets a perspective camera's horizontal field of view (degrees), the way the game's configs
/// give it.
fn set_fov(projection: &mut Projection, horizontal: f32) {
    if let Projection::Perspective(p) = projection {
        let fov = 2.0 * ((horizontal.to_radians() * 0.5).tan() / p.aspect_ratio.max(0.1)).atan();
        if (p.fov - fov).abs() > 1e-4 {
            p.fov = fov;
        }
    }
}

fn draw_trail(trail: Res<Trail>, overlay: Res<DebugOverlay>, mut gizmos: Gizmos) {
    if !overlay.0 {
        return;
    }
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

fn update_hud(
    settings: Res<Settings>,
    hits: Res<combat::Hits>,
    arsenal: Option<Res<weapons::Arsenal>>,
    players: Query<&Player>,
    mut hud: Query<&mut Text, With<HudText>>,
) {
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
         dummies hit: {}  {}\n\
         {}",
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
            .map(|w| format!("hit {:.2} s, free {:.2} s, over {:.2} s, lunge {:.2} m", w.hit_time, w.early_exit, w.exit, w.lunge.last().copied().unwrap_or(0.0)))
            .collect::<Vec<_>>()
            .join(", "),
        t.melee.smash_hop.height_min,
        t.melee.smash_gravity,
        t.melee.smash_radius,
        t.melee.throw.out_speed,
        t.melee.throw.max_distance,
        hits.count,
        if hits.last.is_empty() { String::new() } else { format!("(last: {})", hits.last) },
        arsenal.map_or(String::new(), |a| weapons::hud_line(&a)),
    );
}

//! Weapon effects built from the game's own effect art: the textures and meshes that Rift
//! Apart's Burst Pistol and Shatterbomb effects use, exported from the user's install into
//! `riftapart-assets/vfx/` by `tools/extract_riftapart_vfx.py`, with the colours the extractor
//! read from the game's effect materials and particle data (`vfx.json` "palette"). The game's
//! particle systems themselves aren't decoded, so the timing, sizes and motion here are ours.
//!
//! Without the art the weapons fall back to plain shapes.

use crate::Settings;
use bevy::color::Mix;
use bevy::gltf::GltfAssetLabel;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology};
use std::collections::{HashMap, VecDeque};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::Path;

const TEXTURES: &[&str] = &[
    "tracer",
    "muzzle_flash",
    "glow",
    "impact_flare",
    "spark",
    "bullet_decal",
    "smoke_puff",
    "smoke_trail",
    "shockwave_mask",
    "flare_radial",
    "glow_simple",
    "debris_chunk",
    "scorch_mask",
    "scorch_glow",
    "swipe_wrench",
    "trail_basic",
    "blast_masks",
    "crater",
];
/// Effect meshes and how many primitives each has.
const MESHES: &[(&str, usize)] = &[
    ("tracer_mesh", 1),
    ("projectile_shape", 1),
    ("muzzle_flash_mesh", 1),
    ("shockwave_hemisphere", 1),
    ("impact_blast", 4),
    ("shatter_shards", 5),
    // The Burst Pistol's shell: glass, the energy inside it, the metal ends.
    ("shell", 3),
];
/// How long a bolt's streak takes to catch up with it once it has hit something.
const STREAK_CATCH_UP: f32 = 0.05;
/// How long the wrench's swing streak lingers behind it (ours).
const SWIPE_LIFE: f32 = 0.12;
/// The swing streak runs along the wrench between these distances from the grip (its +Y, metres
/// at Ratchet's size): around its effects locator (igLoc_dyn_fx_attach_00, 0.41) out past the
/// head. Ours, from the streak config's offset and width scale.
const SWIPE_SPAN: (f32, f32) = (0.2, 0.62);

/// The colours the effects use (linear RGB, may exceed 1 to glow). Generic ones unless the
/// extractor's palette has the game's.
struct Colors {
    /// The bolt's core: its projectile mesh colour times that material's intensity.
    bolt: LinearRgba,
    bolt_glow: LinearRgba,
    impact_flash: LinearRgba,
    impact_spark: LinearRgba,
    impact_glow: LinearRgba,
    impact_glow_end: LinearRgba,
    impact_smoke: LinearRgba,
    /// The green impact decal's glowing colour.
    decal_glow: LinearRgba,
    shell_glass: LinearRgba,
    blast_hot: LinearRgba,
    blast_fire: LinearRgba,
    blast_fire_end: LinearRgba,
    blast_energy: LinearRgba,
    blast_shards: LinearRgba,
    blast_smoke: LinearRgba,
    scorch_glow: LinearRgba,
    grenade_glow: LinearRgba,
    /// The OmniWrench: its swing streak (the swipe material's colour times its intensity), hits,
    /// the Hyper-Strike slam and its scorch, and the thrown wrench's trail.
    wrench_swipe: LinearRgba,
    wrench_hit: LinearRgba,
    wrench_smoke: LinearRgba,
    wrench_slam: LinearRgba,
    wrench_slam_glow: LinearRgba,
    slam_scorch: LinearRgba,
    wrench_trail: LinearRgba,
    wrench_trail_end: LinearRgba,
    /// How many came from the game.
    from_game: usize,
}

impl Colors {
    fn load(vfx_json: &Path) -> Colors {
        let palette: HashMap<String, Vec<f32>> = std::fs::read_to_string(vfx_json)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| serde_json::from_value(v["palette"].clone()).ok())
            .unwrap_or_default();
        let mut from_game = 0;
        let mut rgb = |name: &str, or: [f32; 3]| match palette.get(name) {
            Some(v) if v.len() >= 3 => {
                from_game += 1;
                LinearRgba::rgb(v[0], v[1], v[2])
            }
            _ => LinearRgba::rgb(or[0], or[1], or[2]),
        };
        let bolt = rgb("bolt", [0.1, 0.8, 0.1]);
        let colors = Colors {
            bolt,
            bolt_glow: rgb("bolt_glow", [0.4, 0.9, 0.1]),
            impact_flash: rgb("impact_flash", [0.9, 1.0, 0.4]),
            impact_spark: rgb("impact_spark", [0.7, 1.0, 0.3]),
            impact_glow: rgb("impact_glow", [0.4, 0.9, 0.2]),
            impact_glow_end: rgb("impact_glow_end", [0.15, 0.25, 0.1]),
            impact_smoke: rgb("impact_smoke", [0.25, 0.25, 0.25]),
            decal_glow: rgb("decal_glow", [0.2, 0.8, 0.05]),
            shell_glass: rgb("shell_glass", [0.1, 0.2, 0.05]),
            blast_hot: rgb("blast_hot", [1.5, 1.3, 1.0]),
            blast_fire: rgb("blast_fire", [1.0, 0.6, 0.25]),
            blast_fire_end: rgb("blast_fire_end", [1.0, 0.35, 0.05]),
            blast_energy: rgb("blast_energy", [0.5, 0.3, 1.0]),
            blast_shards: rgb("blast_shards", [0.8, 0.1, 1.0]),
            blast_smoke: rgb("blast_smoke", [0.2, 0.22, 0.25]),
            scorch_glow: rgb("scorch_glow", [0.3, 0.1, 0.8]),
            grenade_glow: rgb("grenade_glow", [0.45, 0.1, 0.85]),
            wrench_swipe: rgb("wrench_swipe", [0.05, 0.5, 1.0]),
            wrench_hit: rgb("wrench_hit", [0.2, 0.65, 1.0]),
            wrench_smoke: rgb("wrench_smoke", [0.3, 0.33, 0.35]),
            wrench_slam: rgb("wrench_slam", [0.35, 0.75, 1.0]),
            wrench_slam_glow: rgb("wrench_slam_glow", [0.2, 0.7, 1.0]),
            slam_scorch: rgb("slam_scorch", [0.2, 0.7, 1.0]),
            wrench_trail: rgb("wrench_trail", [0.5, 0.85, 1.0]),
            wrench_trail_end: rgb("wrench_trail_end", [0.05, 0.6, 1.0]),
            from_game: 0,
        };
        let intensity = |name: &str, or: f32| palette.get(name).and_then(|v| v.first().copied()).unwrap_or(or);
        Colors {
            bolt: colors.bolt * intensity("bolt_intensity", 5.0),
            wrench_swipe: colors.wrench_swipe * intensity("wrench_swipe_intensity", 4.0),
            from_game,
            ..colors
        }
    }
}

#[derive(Resource)]
pub struct VfxArt {
    pub available: bool,
    tex: HashMap<&'static str, Handle<Image>>,
    mesh: HashMap<&'static str, Vec<Handle<Mesh>>>,
    quad: Handle<Mesh>,
    colors: Colors,
    /// The shell's materials, shared by every shell.
    shell: [Handle<StandardMaterial>; 3],
}

/// How an effect piece turns each frame.
#[derive(Clone, Copy, PartialEq)]
pub enum Facing {
    /// Keeps its spawn rotation (plus spin).
    Fixed,
    /// Faces the camera.
    Camera,
    /// Stretched along its velocity, turned to the camera (sparks).
    Velocity,
    /// Stretched along a fixed direction, turned to the camera (trails).
    Axis(Vec3),
}

/// One piece of an effect: it moves, grows and fades over its life, then disappears.
#[derive(Component)]
pub struct Fx {
    age: f32,
    life: f32,
    vel: Vec3,
    gravity: f32,
    drag: f32,
    spin: Vec3,
    scale: (Vec3, Vec3),
    /// Alpha at the start and end; the fade starts at `fade_from` (fraction of the life).
    alpha: (f32, f32),
    fade_from: f32,
    color: LinearRgba,
    /// Colour it shifts to over its life.
    color_end: Option<LinearRgba>,
    facing: Facing,
    /// Camera-facing pieces: their turn about the view axis.
    roll: f32,
    /// Bounces off this height (shells, shards, sparks).
    floor: Option<f32>,
    /// Shrinks away at the end of its life instead of fading (solid pieces).
    shrink: bool,
    /// Pieces that carry a point light: its intensity at full alpha.
    light: f32,
}

impl Fx {
    fn new(life: f32) -> Self {
        Fx {
            age: 0.0,
            life,
            vel: Vec3::ZERO,
            gravity: 0.0,
            drag: 0.0,
            spin: Vec3::ZERO,
            scale: (Vec3::ONE, Vec3::ONE),
            alpha: (1.0, 0.0),
            fade_from: 0.0,
            color: LinearRgba::WHITE,
            color_end: None,
            facing: Facing::Fixed,
            roll: 0.0,
            floor: None,
            shrink: false,
            light: 0.0,
        }
    }
}

/// The bright streak behind a bolt: it grows out of the muzzle to its full length and follows
/// the bolt, with a glow at its head that faces the camera (so a bolt flying away still shows);
/// when the bolt hits something it catches up and disappears.
#[derive(Component)]
pub struct Streak {
    bolt: Entity,
    glow: Entity,
    origin: Vec3,
    head: Vec3,
    len: f32,
    width: f32,
    gone: f32,
}

/// The OmniWrench's swing streak: a ribbon through where the wrench has been over the last
/// moment, rebuilt every frame while he swings.
#[derive(Component)]
pub struct SwipeStreak {
    /// Inner and outer edge, and when.
    samples: VecDeque<(Vec3, Vec3, f32)>,
    mesh: Handle<Mesh>,
    /// The mesh is collapsed to nothing (it's never hidden: an entity spawned hidden and shown
    /// later didn't draw its rebuilt mesh).
    collapsed: bool,
}

/// A triangle with no area, for a ribbon with nothing to show (the renderer rejects a mesh
/// without vertices).
fn collapse_ribbon(mesh: &mut Mesh) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
}

/// Random numbers for effect variety (deterministic, so demo runs repeat).
#[derive(Resource)]
pub struct FxRng(u32);

impl FxRng {
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
    pub fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
    pub fn unit(&mut self) -> Vec3 {
        Vec3::new(self.signed(), self.signed(), self.signed()).normalize_or(Vec3::Y)
    }
}

pub fn plugin(app: &mut App) {
    app.insert_resource(FxRng(0x2545_F491)).add_systems(Startup, load).add_systems(Update, (run_fx, run_streaks, run_swipe));
}

fn load(
    mut commands: Commands,
    settings: Res<Settings>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dir = settings.model_dir.as_ref().map(|d| d.join("vfx"));
    let available = dir
        .as_ref()
        .is_some_and(|d| TEXTURES.iter().all(|t| d.join(format!("{t}.png")).exists()) && MESHES.iter().all(|(m, _)| d.join(format!("{m}.glb")).exists()));
    let mut tex = HashMap::new();
    let mut mesh = HashMap::new();
    let colors = Colors::load(&dir.as_ref().map(|d| d.join("vfx.json")).unwrap_or_default());
    if available {
        for &t in TEXTURES {
            tex.insert(t, assets.load(format!("vfx/{t}.png")));
        }
        for &(m, prims) in MESHES {
            let handles = (0..prims).map(|i| assets.load(GltfAssetLabel::Primitive { mesh: i, primitive: 0 }.from_asset(format!("vfx/{m}.glb")))).collect();
            mesh.insert(m, handles);
        }
        info!("weapon effects: using the game's effect art ({} textures, {} meshes, {} colours from its effect data)", tex.len(), mesh.len(), colors.from_game);
    }
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    // The shell: tinted glass, the bolt's green energy inside, steel ends.
    let shell = [
        materials.add(StandardMaterial {
            base_color: (colors.shell_glass * 5.0).with_alpha(0.45).into(),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.08,
            reflectance: 0.6,
            ..default()
        }),
        materials.add(StandardMaterial { base_color: colors.bolt_glow.into(), emissive: colors.bolt * 0.6, unlit: true, ..default() }),
        materials.add(StandardMaterial { base_color: Color::srgb(0.62, 0.63, 0.66), metallic: 0.9, perceptual_roughness: 0.35, ..default() }),
    ];
    if available {
        // The swing streak's ribbon, rebuilt each frame (world-space vertices).
        let mut ribbon = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        collapse_ribbon(&mut ribbon);
        let ribbon = meshes.add(ribbon);
        // Blended rather than added, so the blue holds up in front of a bright floor.
        let swipe = blended(&mut materials, tex.get("swipe_wrench").cloned(), colors.wrench_swipe);
        commands.spawn((
            SwipeStreak { samples: VecDeque::new(), mesh: ribbon.clone(), collapsed: true },
            Mesh3d(ribbon),
            MeshMaterial3d(swipe),
            Transform::default(),
            Visibility::default(),
            NotShadowCaster,
            NoFrustumCulling,
        ));
    }
    commands.insert_resource(VfxArt { available, tex, mesh, quad, colors, shell });
}

// Both take colours scaled up to glow: `LinearRgba * k` scales alpha too, so it's reset to 1
// here (pieces with `Fx` get their alpha from it each frame).
fn additive(materials: &mut Assets<StandardMaterial>, tex: Option<Handle<Image>>, color: LinearRgba) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color.with_alpha(1.0).into(),
        base_color_texture: tex,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

fn blended(materials: &mut Assets<StandardMaterial>, tex: Option<Handle<Image>>, color: LinearRgba) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color.with_alpha(1.0).into(),
        base_color_texture: tex,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

/// A point light that fades with its piece (no shadows).
fn flash_light(color: LinearRgba, intensity: f32, range: f32) -> PointLight {
    PointLight { color: color.into(), intensity, range, shadow_maps_enabled: false, ..default() }
}

/// Long axis (Y) along `axis`, face (Z) turned as far toward the camera as that allows.
fn axis_facing(axis: Vec3, to_cam: Vec3) -> Quat {
    let z = (to_cam - axis * to_cam.dot(axis)).normalize_or(axis.any_orthonormal_vector());
    Quat::from_mat3(&Mat3::from_cols(axis.cross(z), axis, z))
}

impl VfxArt {
    fn tex(&self, name: &str) -> Option<Handle<Image>> {
        self.tex.get(name).cloned()
    }
    fn mesh(&self, name: &str, i: usize) -> Handle<Mesh> {
        self.mesh.get(name).and_then(|v| v.get(i % v.len().max(1))).cloned().unwrap_or_else(|| self.quad.clone())
    }

    /// A camera-facing sprite.
    #[allow(clippy::too_many_arguments)]
    fn sprite(
        &self,
        commands: &mut Commands,
        materials: &mut Assets<StandardMaterial>,
        tex: &str,
        at: Vec3,
        size: (f32, f32),
        color: LinearRgba,
        fx: Fx,
        add: bool,
    ) {
        let mat = if add { additive(materials, self.tex(tex), color) } else { blended(materials, self.tex(tex), color) };
        commands.spawn((
            Fx { scale: (Vec3::splat(size.0), Vec3::splat(size.1)), color, facing: Facing::Camera, ..fx },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(at).with_scale(Vec3::splat(size.0)),
            NotShadowCaster,
        ));
    }

    /// A light that flashes and fades.
    fn light(&self, commands: &mut Commands, at: Vec3, color: LinearRgba, intensity: f32, range: f32, life: f32) {
        commands.spawn((Fx { light: intensity, fade_from: 0.1, ..Fx::new(life) }, flash_light(color, intensity, range), Transform::from_translation(at)));
    }

    /// The light a Shatterbomb gives off in flight.
    pub fn grenade_light(&self, s: f32) -> PointLight {
        flash_light(self.colors.grenade_glow, 20_000.0 * s * s, 2.5 * s)
    }

    /// The Burst Pistol's flash: the game's muzzle-flash mesh pointing along the shot, its flash
    /// shape facing the camera, a glow and a burst of light.
    pub fn muzzle_flash(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, rng: &mut FxRng, at: Vec3, dir: Vec3, s: f32) {
        let c = &self.colors;
        let rot = Quat::from_rotation_arc(Vec3::Z, dir) * Quat::from_rotation_z(rng.next() * TAU);
        let color = c.bolt_glow * 6.0;
        let size = 0.1 * s;
        commands.spawn((
            Fx { scale: (Vec3::splat(size), Vec3::splat(size * 1.6)), fade_from: 0.3, color, ..Fx::new(0.05) },
            Mesh3d(self.mesh("muzzle_flash_mesh", 0)),
            MeshMaterial3d(blended(materials, self.tex("muzzle_flash"), color)),
            Transform::from_translation(at).with_rotation(rot).with_scale(Vec3::splat(size)),
            NotShadowCaster,
        ));
        let flash = Fx { roll: rng.next() * TAU, fade_from: 0.4, ..Fx::new(0.05) };
        self.sprite(commands, materials, "muzzle_flash", at + dir * 0.06 * s, (0.3 * s, 0.45 * s), c.bolt_glow * 4.0, flash, true);
        self.sprite(commands, materials, "glow", at, (0.6 * s, 0.9 * s), c.bolt_glow * 1.5, Fx { fade_from: 0.2, ..Fx::new(0.1) }, true);
        self.light(commands, at + dir * 0.1 * s, c.bolt_glow, 80_000.0 * s * s, 3.0 * s, 0.07);
    }

    /// A bolt's head, as children of the bolt entity (which points +Z along its flight): the
    /// game's tracer mesh, its glow shape and a green light.
    pub fn bolt(&self, parent: &mut ChildSpawnerCommands, materials: &mut Assets<StandardMaterial>, s: f32) {
        let c = &self.colors;
        let core = blended(materials, self.tex("tracer"), c.bolt);
        parent.spawn((Mesh3d(self.mesh("tracer_mesh", 0)), MeshMaterial3d(core), Transform::from_scale(Vec3::new(1.2, 1.2, 1.8) * s), NotShadowCaster));
        let glow = additive(materials, self.tex("glow_simple"), c.bolt_glow * 1.5);
        parent.spawn((Mesh3d(self.mesh("projectile_shape", 0)), MeshMaterial3d(glow), Transform::from_scale(Vec3::new(0.12, 0.12, 0.4) * s), NotShadowCaster));
        parent.spawn((flash_light(c.bolt_glow, 25_000.0 * s * s, 2.5 * s), Transform::default()));
    }

    /// The streak that follows the bolt `bolt` out of the muzzle at `origin`.
    pub fn streak(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, bolt: Entity, origin: Vec3, s: f32) {
        let glow = commands
            .spawn((
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(additive(materials, self.tex("glow"), self.colors.bolt_glow * 4.0)),
                Transform::from_translation(origin).with_scale(Vec3::ZERO),
                NotShadowCaster,
            ))
            .id();
        let mat = additive(materials, self.tex("spark"), self.colors.bolt_glow * 6.0);
        commands.spawn((
            Streak { bolt, glow, origin, head: origin, len: 2.0 * s, width: 0.14 * s, gone: 0.0 },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(origin).with_scale(Vec3::ZERO),
            NotShadowCaster,
        ));
    }

    /// The bolt's smoke trail over one step of its flight (a → b): the game's trail texture,
    /// lingering a moment.
    pub fn bolt_trail(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, a: Vec3, b: Vec3, s: f32) {
        let c = &self.colors;
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let color = c.bolt_glow * 1.5;
        commands.spawn((
            Fx {
                scale: (Vec3::new(0.06 * s, len, 1.0), Vec3::new(0.22 * s, len, 1.0)),
                alpha: (1.0, 0.0),
                color,
                color_end: Some(c.impact_glow_end),
                facing: Facing::Axis(d / len),
                ..Fx::new(0.35)
            },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(additive(materials, self.tex("smoke_trail"), color)),
            Transform::from_translation((a + b) * 0.5),
            NotShadowCaster,
        ));
    }

    /// A spent shell thrown out of the gun's ejector (at `at`, whose Z axis points out of the
    /// gun), tumbling and bouncing on the floor.
    #[allow(clippy::too_many_arguments)]
    pub fn shell(&self, commands: &mut Commands, rng: &mut FxRng, at: Vec3, rot: Quat, params: &libarsenal::ShellParams, floor: Option<f32>, s: f32) {
        let speed = params.speed.0 + (params.speed.1 - params.speed.0) * rng.next();
        let vel = rot * Vec3::Z * speed + rng.unit() * 0.3 * s;
        let spin = rot * (params.spin * Vec3::new(rng.signed(), rng.signed(), rng.signed()));
        commands
            .spawn((
                Fx {
                    vel,
                    gravity: 9.8 * s,
                    spin,
                    scale: (Vec3::splat(s), Vec3::splat(s)),
                    alpha: (1.0, 1.0),
                    floor: floor.map(|y| y + 0.02 * s),
                    shrink: true,
                    ..Fx::new(1.6 + 0.4 * rng.next())
                },
                Transform::from_translation(at).with_rotation(rot).with_scale(Vec3::splat(s)),
                Visibility::default(),
            ))
            .with_children(|c| {
                for (i, mat) in self.shell.iter().enumerate() {
                    let mut e = c.spawn((Mesh3d(self.mesh("shell", i)), MeshMaterial3d(mat.clone())));
                    if i < 2 {
                        e.insert(NotShadowCaster);
                    }
                }
            });
    }

    /// Where a bolt hits: the impact flare, a glow and a flash of light, sparks bouncing off the
    /// surface, a puff of smoke, and a mark on level geometry that glows green for a moment.
    #[allow(clippy::too_many_arguments)]
    pub fn bolt_impact(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, rng: &mut FxRng, at: Vec3, normal: Vec3, decal: bool, s: f32) {
        let c = &self.colors;
        let off = at + normal * 0.05 * s;
        let flare = Fx { roll: rng.next() * TAU, fade_from: 0.1, ..Fx::new(0.1) };
        self.sprite(commands, materials, "impact_flare", off, (0.4 * s, 1.1 * s), c.impact_flash * 6.0, flare, false);
        let glow = Fx { color_end: Some(c.impact_glow_end), ..Fx::new(0.2) };
        self.sprite(commands, materials, "glow", off, (0.6 * s, 1.0 * s), c.impact_glow * 2.0, glow, true);
        self.light(commands, at + normal * 0.3 * s, c.impact_glow, 40_000.0 * s * s, 2.5 * s, 0.12);
        let floor = (normal.y > 0.5).then_some(at.y);
        for _ in 0..9 {
            let dir = (normal + rng.unit() * 0.9).normalize_or(normal);
            let color = c.impact_spark * 5.0;
            let w = 0.025 * s;
            commands.spawn((
                Fx {
                    vel: dir * (3.0 + 5.0 * rng.next()) * s,
                    gravity: 9.0 * s,
                    drag: 1.5,
                    scale: (Vec3::new(w, w * 6.0, 1.0), Vec3::new(w * 0.5, w * 3.0, 1.0)),
                    fade_from: 0.4,
                    color,
                    color_end: Some(c.impact_glow_end * 3.0),
                    facing: Facing::Velocity,
                    floor,
                    ..Fx::new(0.25 + 0.2 * rng.next())
                },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(blended(materials, self.tex("spark"), color)),
                Transform::from_translation(off),
                NotShadowCaster,
            ));
        }
        let smoke = Fx { vel: (normal * 0.4 + Vec3::Y * 0.6) * s, drag: 0.5, alpha: (0.4, 0.0), fade_from: 0.2, roll: rng.next() * TAU, ..Fx::new(0.7) };
        self.sprite(commands, materials, "smoke_puff", off, (0.25 * s, 0.8 * s), c.impact_smoke * 2.0, smoke, false);
        if decal {
            let rot = Quat::from_rotation_arc(Vec3::Z, normal) * Quat::from_rotation_z(rng.next() * TAU);
            let size = 0.22 * s;
            let mark = LinearRgba::rgb(0.3, 0.32, 0.3);
            commands.spawn((
                Fx { scale: (Vec3::splat(size), Vec3::splat(size)), alpha: (0.9, 0.0), fade_from: 0.8, color: mark, ..Fx::new(8.0) },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(blended(materials, self.tex("bullet_decal"), mark)),
                Transform::from_translation(at + normal * 0.004).with_rotation(rot).with_scale(Vec3::splat(size)),
                NotShadowCaster,
            ));
            let hot = c.decal_glow * 4.0;
            commands.spawn((
                Fx { scale: (Vec3::splat(size * 0.8), Vec3::splat(size * 0.6)), fade_from: 0.1, color: hot, ..Fx::new(0.8) },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(additive(materials, self.tex("glow_simple"), hot)),
                Transform::from_translation(at + normal * 0.006).with_rotation(rot).with_scale(Vec3::splat(size * 0.8)),
                NotShadowCaster,
            ));
        }
    }

    /// A wrench hit: a blue flash with an anamorphic streak, a burst of light, sparks spraying back
    /// off the target, and a puff of smoke. `dir` points from Ratchet to the target.
    #[allow(clippy::too_many_arguments)]
    pub fn wrench_hit(
        &self,
        commands: &mut Commands,
        materials: &mut Assets<StandardMaterial>,
        rng: &mut FxRng,
        at: Vec3,
        dir: Vec3,
        floor: Option<f32>,
        s: f32,
    ) {
        let c = &self.colors;
        let flash = Fx { roll: rng.next() * TAU, fade_from: 0.1, ..Fx::new(0.12) };
        self.sprite(commands, materials, "flare_radial", at, (0.5 * s, 1.4 * s), c.wrench_hit * 4.0, flash, false);
        let streak = c.wrench_hit * 3.0;
        commands.spawn((
            Fx { scale: (Vec3::new(0.6 * s, 0.08 * s, 1.0), Vec3::new(1.8 * s, 0.03 * s, 1.0)), color: streak, facing: Facing::Camera, ..Fx::new(0.15) },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(additive(materials, self.tex("glow"), streak)),
            Transform::from_translation(at),
            NotShadowCaster,
        ));
        self.light(commands, at - dir * 0.3 * s, c.wrench_hit, 60_000.0 * s * s, 3.0 * s, 0.12);
        for _ in 0..10 {
            let v = (-dir + rng.unit() * 0.9 + Vec3::Y * 0.3).normalize_or(Vec3::Y);
            let color = c.wrench_hit * 5.0;
            let w = 0.02 * s;
            commands.spawn((
                Fx {
                    vel: v * (3.0 + 5.0 * rng.next()) * s,
                    gravity: 9.0 * s,
                    drag: 1.5,
                    scale: (Vec3::new(w, w * 7.0, 1.0), Vec3::new(w * 0.5, w * 3.0, 1.0)),
                    fade_from: 0.4,
                    color,
                    color_end: Some(c.wrench_trail_end * 3.0),
                    facing: Facing::Velocity,
                    floor,
                    ..Fx::new(0.25 + 0.15 * rng.next())
                },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(blended(materials, self.tex("spark"), color)),
                Transform::from_translation(at),
                NotShadowCaster,
            ));
        }
        let smoke = Fx { vel: (Vec3::Y * 0.6 - dir * 0.3) * s, drag: 0.5, alpha: (0.35, 0.0), fade_from: 0.2, roll: rng.next() * TAU, ..Fx::new(0.6) };
        self.sprite(commands, materials, "smoke_puff", at, (0.3 * s, 0.8 * s), c.wrench_smoke * 2.0, smoke, false);
    }

    /// The Hyper-Strike landing at `at` (on the ground): a flash and a burst of light, the game's
    /// impact-blast shapes and blast masks bursting up, a ring along the ground, dust and
    /// debris, and its crater with blue embers.
    #[allow(clippy::too_many_arguments)]
    pub fn wrench_slam(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, rng: &mut FxRng, at: Vec3, radius: f32, s: f32) {
        let c = &self.colors;
        let up = at + Vec3::Y * 0.2 * s;
        self.light(commands, at + Vec3::Y * 0.8 * s, c.wrench_slam, 1.2e6 * s * s, radius * 3.0, 0.4);
        let flash = Fx { roll: rng.next() * TAU, fade_from: 0.1, ..Fx::new(0.15) };
        self.sprite(commands, materials, "flare_radial", up, (1.5 * s, 4.0 * s), c.wrench_slam * 6.0, flash, true);
        self.sprite(commands, materials, "glow_simple", up, (radius * 0.5, radius * 1.2), c.wrench_slam_glow * 2.5, Fx { fade_from: 0.1, ..Fx::new(0.35) }, true);
        // The game's impact-blast shapes, bursting up.
        for i in 0..4 {
            let color = c.wrench_slam * 2.0;
            let size = 1.4 * s;
            commands.spawn((
                Fx { scale: (Vec3::splat(size * 0.3), Vec3::splat(size)), fade_from: 0.2, color, ..Fx::new(0.3) },
                Mesh3d(self.mesh("impact_blast", i)),
                MeshMaterial3d(additive(materials, None, color)),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(rng.next() * TAU)).with_scale(Vec3::splat(size * 0.3)),
                NotShadowCaster,
            ));
        }
        // Blast masks around the ring, rising (the texture's burst points down: turned over).
        for k in 0..6 {
            let a = k as f32 / 6.0 * TAU + rng.next() * 0.4;
            let p = at + Vec3::new(a.cos(), 0.0, a.sin()) * 0.6 * s + Vec3::Y * 0.4 * s;
            let fx = Fx { vel: Vec3::Y * 1.5 * s, drag: 2.0, roll: PI + rng.signed() * 0.3, fade_from: 0.3, ..Fx::new(0.3) };
            self.sprite(commands, materials, "blast_masks", p, (0.6 * s, 1.6 * s), c.wrench_slam * 2.0, fx, false);
        }
        // A ring along the ground: the game's shockwave dome, flattened.
        let ring = c.wrench_slam * 2.0;
        commands.spawn((
            Fx { scale: (Vec3::new(radius * 0.2, radius * 0.2, 0.05 * s), Vec3::new(radius, radius, 0.3 * s)), fade_from: 0.2, color: ring, ..Fx::new(0.35) },
            Mesh3d(self.mesh("shockwave_hemisphere", 0)),
            MeshMaterial3d(additive(materials, self.tex("shockwave_mask"), ring)),
            Transform::from_translation(at).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
            NotShadowCaster,
        ));
        // Dust rolling out along the ground, and debris.
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU + rng.next() * 0.5;
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            let fx = Fx {
                vel: (out * (2.5 + 1.5 * rng.next()) + Vec3::Y * 0.4) * s,
                drag: 2.0,
                alpha: (0.5, 0.0),
                fade_from: 0.25,
                roll: rng.next() * TAU,
                ..Fx::new(0.9 + 0.4 * rng.next())
            };
            self.sprite(commands, materials, "smoke_puff", at + out * 0.4 * s + Vec3::Y * 0.2 * s, (0.6 * s, 1.6 * s), c.wrench_smoke * 3.0, fx, false);
        }
        for _ in 0..10 {
            let dir = Vec3::new(rng.signed(), 1.0 + rng.next(), rng.signed()).normalize();
            let fx = Fx {
                vel: dir * (4.0 + 4.0 * rng.next()) * s,
                gravity: 16.0 * s,
                floor: Some(at.y + 0.03 * s),
                alpha: (1.0, 0.0),
                fade_from: 0.7,
                roll: rng.next() * TAU,
                ..Fx::new(0.8 + 0.4 * rng.next())
            };
            self.sprite(commands, materials, "debris_chunk", up, (0.1 * s, 0.1 * s), LinearRgba::rgb(0.45, 0.42, 0.4), fx, false);
        }
        // The crater, and blue embers cooling in it.
        let flat = Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_z(rng.next() * TAU);
        let size = radius * 0.7;
        let mark = LinearRgba::rgb(0.6, 0.6, 0.6);
        commands.spawn((
            Fx { scale: (Vec3::splat(size), Vec3::splat(size)), alpha: (0.9, 0.0), fade_from: 0.85, color: mark, ..Fx::new(10.0) },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(blended(materials, self.tex("crater"), mark)),
            Transform::from_translation(at + Vec3::Y * 0.006).with_rotation(flat).with_scale(Vec3::splat(size)),
            NotShadowCaster,
        ));
        let embers = c.slam_scorch * 5.0;
        commands.spawn((
            Fx { scale: (Vec3::splat(size), Vec3::splat(size)), fade_from: 0.15, color: embers, ..Fx::new(1.5) },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(additive(materials, self.tex("scorch_glow"), embers)),
            Transform::from_translation(at + Vec3::Y * 0.008).with_rotation(flat).with_scale(Vec3::splat(size)),
            NotShadowCaster,
        ));
    }

    /// The thrown wrench's trail over one step of its flight (a → b), and its glow.
    pub fn wrench_trail(&self, commands: &mut Commands, materials: &mut Assets<StandardMaterial>, a: Vec3, b: Vec3, s: f32) {
        let c = &self.colors;
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let color = c.wrench_trail * 3.0;
        commands.spawn((
            Fx {
                scale: (Vec3::new(0.5 * s, len, 1.0), Vec3::new(0.15 * s, len, 1.0)),
                alpha: (0.95, 0.0),
                color,
                color_end: Some(c.wrench_trail_end * 2.0),
                facing: Facing::Axis(d / len),
                ..Fx::new(0.4)
            },
            Mesh3d(self.quad.clone()),
            MeshMaterial3d(blended(materials, self.tex("trail_basic"), color)),
            Transform::from_translation((a + b) * 0.5),
            NotShadowCaster,
        ));
    }

    /// The Shatterbomb going off: a burst of light, a white-hot flash, a fireball, the game's
    /// shockwave dome in rift violet, its crystal shards flying out, smoke, debris, and a scorch
    /// mark with violet embers.
    #[allow(clippy::too_many_arguments)]
    pub fn explosion(
        &self,
        commands: &mut Commands,
        materials: &mut Assets<StandardMaterial>,
        rng: &mut FxRng,
        at: Vec3,
        radius: f32,
        ground: Option<f32>,
        s: f32,
    ) {
        let c = &self.colors;
        self.light(commands, at + Vec3::Y * 0.6 * s, c.blast_fire, 3.0e6 * s * s, radius * 3.0, 0.5);
        let flash = Fx { roll: rng.next() * TAU, fade_from: 0.1, ..Fx::new(0.16) };
        self.sprite(commands, materials, "flare_radial", at, (2.5 * s, 7.0 * s), c.blast_hot * 6.0, flash, true);
        // The fireball: a hot core and puffs of the game's smoke texture burning orange to deep
        // orange.
        self.sprite(commands, materials, "glow", at, (1.5 * s, 3.5 * s), c.blast_fire * 4.0, Fx { fade_from: 0.2, ..Fx::new(0.3) }, true);
        for _ in 0..8 {
            let dir = Vec3::new(rng.signed(), 0.3 + rng.next(), rng.signed()).normalize();
            // Blended rather than added, so the fire stays orange in front of a bright sky.
            let fx = Fx {
                vel: dir * (3.0 + 3.0 * rng.next()) * s,
                drag: 3.5,
                alpha: (0.95, 0.0),
                color_end: Some(c.blast_fire_end * 2.5),
                fade_from: 0.35,
                roll: rng.next() * TAU,
                ..Fx::new(0.3 + 0.2 * rng.next())
            };
            self.sprite(commands, materials, "smoke_puff", at + dir * 0.3 * s, (1.1 * s, 2.8 * s), c.blast_fire * 8.0, fx, false);
        }
        // The game's shockwave dome with its noise mask, in the explosion's violet.
        let dome = c.blast_energy * 2.0;
        let r = radius * 0.45;
        commands.spawn((
            Fx { scale: (Vec3::splat(r * 0.2), Vec3::splat(r)), alpha: (0.7, 0.0), fade_from: 0.1, color: dome, ..Fx::new(0.25) },
            Mesh3d(self.mesh("shockwave_hemisphere", 0)),
            MeshMaterial3d(blended(materials, self.tex("shockwave_mask"), dome)),
            Transform::from_translation(at).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)).with_scale(Vec3::splat(r * 0.15)),
            NotShadowCaster,
        ));
        self.sprite(commands, materials, "glow_simple", at, (radius * 0.4, radius * 1.2), c.blast_energy * 2.5, Fx { fade_from: 0.1, ..Fx::new(0.35) }, true);
        // The grenade shatters: its crystal shards fly out, spinning, glowing violet.
        let floor = ground.map(|y| y + 0.03 * s);
        for i in 0..12 {
            let dir = Vec3::new(rng.signed(), 0.5 + rng.next(), rng.signed()).normalize();
            let mat = materials.add(StandardMaterial {
                base_color: (c.blast_shards * 0.6).with_alpha(0.9).into(),
                emissive: c.blast_shards * 4.0,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.15,
                ..default()
            });
            let size = (0.6 + 0.4 * rng.next()) * s;
            commands.spawn((
                Fx {
                    vel: dir * (6.0 + 6.0 * rng.next()) * s,
                    gravity: 14.0 * s,
                    drag: 0.4,
                    spin: rng.unit() * 12.0,
                    scale: (Vec3::splat(size), Vec3::splat(size)),
                    alpha: (0.9, 0.9),
                    floor,
                    shrink: true,
                    ..Fx::new(0.9 + 0.4 * rng.next())
                },
                Mesh3d(self.mesh("shatter_shards", i)),
                MeshMaterial3d(mat),
                Transform::from_translation(at).with_rotation(Quat::from_scaled_axis(rng.unit() * 3.0)).with_scale(Vec3::splat(size)),
                NotShadowCaster,
            ));
        }
        // Smoke, rising and spreading slowly.
        for _ in 0..6 {
            let dir = Vec3::new(rng.signed(), 0.0, rng.signed()).normalize_or(Vec3::X);
            let fx = Fx {
                vel: (dir * 0.8 + Vec3::Y * (0.5 + 0.5 * rng.next())) * s,
                drag: 0.8,
                gravity: -0.3 * s,
                alpha: (0.45, 0.0),
                fade_from: 0.2,
                roll: rng.next() * TAU,
                ..Fx::new(1.4 + 0.8 * rng.next())
            };
            self.sprite(commands, materials, "smoke_puff", at + dir * 0.4 * s + Vec3::Y * 0.3 * s, (1.2 * s, 3.2 * s), c.blast_smoke * 3.0, fx, false);
        }
        // Debris chunks.
        for _ in 0..10 {
            let dir = Vec3::new(rng.signed(), 0.8 + rng.next(), rng.signed()).normalize();
            let fx = Fx {
                vel: dir * (5.0 + 5.0 * rng.next()) * s,
                gravity: 16.0 * s,
                floor,
                alpha: (1.0, 0.0),
                fade_from: 0.7,
                roll: rng.next() * TAU,
                ..Fx::new(0.8 + 0.4 * rng.next())
            };
            self.sprite(commands, materials, "debris_chunk", at, (0.12 * s, 0.12 * s), LinearRgba::rgb(0.45, 0.42, 0.4), fx, false);
        }
        // A scorch mark, with violet embers cooling on it.
        if let Some(y) = ground {
            let flat = Quat::from_rotation_x(-FRAC_PI_2) * Quat::from_rotation_z(rng.next() * TAU);
            let size = radius * 0.9;
            let dark = LinearRgba::rgb(0.03, 0.025, 0.03);
            commands.spawn((
                Fx { scale: (Vec3::splat(size), Vec3::splat(size)), alpha: (0.8, 0.0), fade_from: 0.85, color: dark, ..Fx::new(10.0) },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(blended(materials, self.tex("scorch_mask"), dark)),
                Transform::from_translation(Vec3::new(at.x, y + 0.006, at.z)).with_rotation(flat).with_scale(Vec3::splat(size)),
                NotShadowCaster,
            ));
            let embers = c.scorch_glow * 6.0;
            commands.spawn((
                Fx { scale: (Vec3::splat(size), Vec3::splat(size)), fade_from: 0.15, color: embers, ..Fx::new(2.5) },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(additive(materials, self.tex("scorch_glow"), embers)),
                Transform::from_translation(Vec3::new(at.x, y + 0.008, at.z)).with_rotation(flat).with_scale(Vec3::splat(size)),
                NotShadowCaster,
            ));
            let glow = c.scorch_glow * 1.5;
            commands.spawn((
                Fx { scale: (Vec3::splat(size * 0.5), Vec3::splat(size * 0.8)), color: glow, ..Fx::new(0.9) },
                Mesh3d(self.quad.clone()),
                MeshMaterial3d(additive(materials, self.tex("glow_simple"), glow)),
                Transform::from_translation(Vec3::new(at.x, y + 0.01, at.z)).with_rotation(flat).with_scale(Vec3::splat(size * 0.5)),
                NotShadowCaster,
            ));
        }
    }
}

/// An effect piece: a mesh with its own material, or a light (or neither, holding children).
type FxPiece<'a> = (Entity, &'a mut Fx, &'a mut Transform, Option<&'a MeshMaterial3d<StandardMaterial>>, Option<&'a mut PointLight>);

fn run_fx(
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut q: Query<FxPiece>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let cam = cams.single().ok().map(|c| (c.translation(), c.rotation()));
    for (e, mut fx, mut tf, mat, light) in &mut q {
        fx.age += dt;
        if fx.age >= fx.life {
            commands.entity(e).despawn();
            continue;
        }
        let t = fx.age / fx.life;
        let drag = (1.0 - fx.drag * dt).max(0.0);
        let g = fx.gravity;
        fx.vel = fx.vel * drag - Vec3::Y * g * dt;
        tf.translation += fx.vel * dt;
        if let Some(floor) = fx.floor
            && tf.translation.y < floor
        {
            tf.translation.y = floor;
            if fx.vel.y < 0.0 {
                fx.vel = Vec3::new(fx.vel.x * 0.5, -fx.vel.y * 0.35, fx.vel.z * 0.5);
                fx.spin *= 0.5;
            }
        }
        let mut scale = fx.scale.0.lerp(fx.scale.1, 1.0 - (1.0 - t) * (1.0 - t));
        if fx.shrink && t > 0.85 {
            scale *= (1.0 - t) / 0.15;
        }
        tf.scale = scale;
        match (fx.facing, cam) {
            (Facing::Camera, Some((_, rot))) => tf.rotation = rot * Quat::from_rotation_z(fx.roll),
            (Facing::Velocity, Some((cam_pos, _))) => tf.rotation = axis_facing(fx.vel.normalize_or(Vec3::Y), cam_pos - tf.translation),
            (Facing::Axis(axis), Some((cam_pos, _))) => tf.rotation = axis_facing(axis, cam_pos - tf.translation),
            _ => {
                if fx.spin != Vec3::ZERO {
                    let spin = Quat::from_scaled_axis(fx.spin * dt);
                    tf.rotation = spin * tf.rotation;
                }
            }
        }
        let a = if t < fx.fade_from { fx.alpha.0 } else { fx.alpha.0 + (fx.alpha.1 - fx.alpha.0) * ((t - fx.fade_from) / (1.0 - fx.fade_from).max(1e-4)) };
        let a = a.clamp(0.0, 1.0);
        if let Some(mut light) = light {
            light.intensity = fx.light * a;
        }
        if let Some(mat) = mat
            && let Some(mut m) = materials.get_mut(&mat.0)
        {
            let c = fx.color_end.map_or(fx.color, |end| fx.color.mix(&end, t));
            // Additive pieces fade by dimming; blended ones by transparency.
            let k = if m.alpha_mode == AlphaMode::Add { a } else { 1.0 };
            m.base_color = Color::LinearRgba(LinearRgba::new(c.red * k, c.green * k, c.blue * k, a));
        }
    }
}

/// Rebuilds the swing streak from where the held wrench has been while he swings.
fn run_swipe(
    time: Res<Time>,
    wrench: Res<crate::wrench::Wrench>,
    players: Query<&crate::Player>,
    globals: Query<&GlobalTransform>,
    mut q: Query<&mut SwipeStreak>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(mut streak) = q.single_mut() else { return };
    let now = time.elapsed_secs();
    let swinging = players.single().is_ok_and(|p| matches!(p.sim.state(), libratchet::MoveState::Melee | libratchet::MoveState::Smash | libratchet::MoveState::SmashLand));
    if swinging && let Some(g) = wrench.held().and_then(|e| globals.get(e).ok()) {
        streak.samples.push_back((g.transform_point(Vec3::Y * SWIPE_SPAN.0), g.transform_point(Vec3::Y * SWIPE_SPAN.1), now));
    }
    while streak.samples.front().is_some_and(|(_, _, t)| now - t > SWIPE_LIFE) {
        streak.samples.pop_front();
    }
    let n = streak.samples.len();
    if n < 2 && streak.collapsed {
        return;
    }
    let Some(mut mesh) = meshes.get_mut(&streak.mesh) else { return };
    streak.collapsed = n < 2;
    if n < 2 {
        collapse_ribbon(&mut mesh);
        return;
    }
    // Two vertices per sample: u across the wrench (inner → outer), v by age (the swipe texture
    // is brightest at v = 0), fading out with age (vertex alpha).
    let (mut pos, mut uv, mut col, mut idx) = (Vec::with_capacity(2 * n), Vec::with_capacity(2 * n), Vec::with_capacity(2 * n), Vec::new());
    for (i, &(inner, outer, t)) in streak.samples.iter().enumerate() {
        let age = ((now - t) / SWIPE_LIFE).clamp(0.0, 1.0);
        let a = 1.0 - age;
        pos.extend([inner.to_array(), outer.to_array()]);
        uv.extend([[0.0, age], [1.0, age]]);
        col.extend([[1.0, 1.0, 1.0, a]; 2]);
        if i > 0 {
            let k = (2 * i) as u32;
            idx.extend([k - 2, k - 1, k, k - 1, k + 1, k]);
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 2 * n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
}

fn run_streaks(
    time: Res<Time>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut transforms: Query<&mut Transform, Without<Streak>>,
    mut q: Query<(Entity, &mut Streak, &mut Transform)>,
    mut commands: Commands,
) {
    let (cam, cam_rot) = cams.single().map_or((Vec3::ZERO, Quat::IDENTITY), |c| (c.translation(), c.rotation()));
    for (e, mut st, mut tf) in &mut q {
        match transforms.get(st.bolt) {
            Ok(head) => st.head = head.translation,
            Err(_) => st.gone += time.delta_secs(),
        }
        let k = 1.0 - st.gone / STREAK_CATCH_UP;
        if k <= 0.0 {
            commands.entity(e).despawn();
            commands.entity(st.glow).despawn();
            continue;
        }
        if let Ok(mut glow) = transforms.get_mut(st.glow) {
            *glow = Transform::from_translation(st.head).with_rotation(cam_rot).with_scale(Vec3::splat(st.width * 5.0 * k));
        }
        let to = st.head - st.origin;
        let dist = to.length();
        let len = dist.min(st.len) * k;
        if len < 1e-3 {
            tf.scale = Vec3::ZERO;
            continue;
        }
        let axis = to / dist;
        tf.translation = st.head - axis * len * 0.5;
        tf.rotation = axis_facing(axis, cam - tf.translation);
        tf.scale = Vec3::new(st.width, len, 1.0);
    }
}

//! The test course and its collision: axis-aligned boxes only, which keeps the collision
//! simple and the measurements easy to read.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
}

impl Block {
    fn new(center_xz: Vec2, bottom: f32, size: Vec3) -> Block {
        let min = Vec3::new(center_xz.x - size.x * 0.5, bottom, center_xz.y - size.z * 0.5);
        Block { min, max: min + size }
    }
}

#[derive(Resource, Default)]
pub struct Level {
    pub blocks: Vec<Block>,
}

/// Where the course pieces are, for respawn and the demo script.
pub mod spots {
    use bevy::prelude::*;
    pub const SPAWN: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    /// Open strip for side-view demos, running along +X.
    pub const DEMO_LANE: Vec3 = Vec3::new(-45.0, 0.0, 100.0);
    /// Top of the glide tower.
    pub const TOWER_TOP: Vec3 = Vec3::new(-30.0, 12.0, 60.0);
}

const FLOOR: f32 = 300.0;

pub fn spawn_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut level = Level::default();

    // Floor: one big block with a 1 m checker so distances can be read off screenshots.
    level.blocks.push(Block { min: Vec3::new(-FLOOR, -1.0, -FLOOR), max: Vec3::new(FLOOR, 0.0, FLOOR) });
    let checker = images.add(checker_image());
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(FLOOR * 2.0, FLOOR * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(checker),
            perceptual_roughness: 0.95,
            uv_transform: Affine2::from_scale(Vec2::splat(FLOOR)),
            ..default()
        })),
    ));

    let mut add = |commands: &mut Commands, block: Block, color: Color| {
        let size = block.max - block.min;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.8, ..default() })),
            Transform::from_translation((block.min + block.max) * 0.5),
        ));
        level.blocks.push(block);
    };

    // Measuring pole next to the spawn: 0.5 m bands, white/red, up to 6 m.
    for i in 0..12 {
        let color = if i % 2 == 0 { Color::srgb(0.95, 0.95, 0.95) } else { Color::srgb(0.85, 0.15, 0.15) };
        add(&mut commands, Block::new(Vec2::new(2.5, -2.0), i as f32 * 0.5, Vec3::new(0.3, 0.5, 0.3)), color);
    }

    // Height steps: pillars of 0.5..3.5 m. Single jump, double jump and beyond.
    for (i, h) in [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5].into_iter().enumerate() {
        let shade = 0.35 + 0.08 * i as f32;
        add(&mut commands, Block::new(Vec2::new(8.0 + i as f32 * 3.5, -8.0), 0.0, Vec3::new(2.0, h, 2.0)), Color::srgb(0.2, shade, 0.9));
    }

    // Gap course: 1 m high platforms with growing gaps (2, 4, 6, 8, 10, 12 m) heading -Z.
    let mut z = -4.0;
    for gap in [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0] {
        z -= gap;
        add(&mut commands, Block::new(Vec2::new(-6.0, z - 1.5), 0.0, Vec3::new(3.0, 1.0, 3.0)), Color::srgb(0.95, 0.65, 0.15));
        z -= 3.0;
    }

    // Low ceiling: walk under a 2.2 m roof and try to jump.
    add(&mut commands, Block::new(Vec2::new(12.0, 6.0), 2.2, Vec3::new(6.0, 0.4, 6.0)), Color::srgb(0.5, 0.5, 0.55));
    for (x, zz) in [(9.2, 3.2), (14.8, 3.2), (9.2, 8.8), (14.8, 8.8)] {
        add(&mut commands, Block::new(Vec2::new(x, zz), 0.0, Vec3::new(0.4, 2.2, 0.4)), Color::srgb(0.4, 0.4, 0.45));
    }

    // Glide tower: 12 m high, with a staircase up its +Z side (0.3 m steps, walkable) and
    // landing pads 10/20/30 m out along +X.
    let top = spots::TOWER_TOP;
    add(&mut commands, Block::new(Vec2::new(top.x, top.z), 0.0, Vec3::new(4.0, top.y, 4.0)), Color::srgb(0.55, 0.45, 0.75));
    const STEPS: usize = 40;
    for i in 0..STEPS {
        let z = top.z + 2.3 + (STEPS - 1 - i) as f32 * 0.6;
        let shade = 0.5 + 0.2 * (i % 2) as f32;
        add(&mut commands, Block::new(Vec2::new(top.x, z), i as f32 * 0.3, Vec3::new(2.0, 0.3, 0.6)), Color::srgb(shade, shade * 0.9, 0.7));
    }
    for d in [10.0, 20.0, 30.0] {
        add(&mut commands, Block::new(Vec2::new(top.x + d, top.z), 0.0, Vec3::new(3.0, 0.2, 3.0)), Color::srgb(0.2, 0.8, 0.4));
    }

    commands.insert_resource(level);
}

fn checker_image() -> Image {
    const N: usize = 2;
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let v = if (x + y) % 2 == 0 { 200 } else { 160 };
            data.extend_from_slice(&[v, v, v + 12, 255]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

/// Result of moving the character's box through the level.
pub struct MoveResult {
    pub feet: Vec3,
    pub grounded: bool,
    pub hit_ceiling: bool,
    pub hit_wall: bool,
}

/// Moves a box (feet position, half width, height) by `delta`, one axis at a time, stopping at
/// blocks and stepping up small ledges.
pub fn move_box(level: &Level, feet: Vec3, half_w: f32, height: f32, delta: Vec3, step_height: f32) -> MoveResult {
    const SKIN: f32 = 1e-4;
    let mut p = feet;
    let mut grounded = false;
    let mut hit_ceiling = false;
    let mut hit_wall = false;

    // Vertical.
    let target_y = p.y + delta.y;
    let mut new_y = target_y;
    for b in &level.blocks {
        if !overlaps_xz(b, p, half_w) {
            continue;
        }
        if delta.y <= 0.0 && p.y >= b.max.y - SKIN && target_y < b.max.y {
            if b.max.y > new_y {
                new_y = b.max.y;
                grounded = true;
            }
        } else if delta.y > 0.0 && p.y + height <= b.min.y + SKIN && target_y + height > b.min.y {
            new_y = new_y.min(b.min.y - height);
            hit_ceiling = true;
        }
    }
    p.y = new_y;

    // Horizontal, X then Z.
    for axis in [0usize, 2] {
        let d = delta[axis];
        if d == 0.0 {
            continue;
        }
        let mut next = p;
        next[axis] += d;
        for b in &level.blocks {
            if !overlaps_box(b, next, half_w, height) {
                continue;
            }
            // Step up onto low ledges instead of stopping.
            let rise = b.max.y - p.y;
            if rise > 0.0 && rise <= step_height && !blocked_above(level, Vec3::new(next.x, b.max.y, next.z), half_w, height) {
                next.y = b.max.y;
                grounded = true;
                continue;
            }
            hit_wall = true;
            next[axis] = if d > 0.0 { b.min[axis] - half_w - SKIN } else { b.max[axis] + half_w + SKIN };
        }
        p = next;
    }

    // Standing exactly on a top surface counts as grounded even without moving down.
    if !grounded && delta.y <= 0.0 {
        grounded = level.blocks.iter().any(|b| overlaps_xz(b, p, half_w) && (p.y - b.max.y).abs() <= 1e-3);
    }

    MoveResult { feet: p, grounded, hit_ceiling, hit_wall }
}

/// Distance from the feet straight down to the nearest top surface.
pub fn ground_distance(level: &Level, feet: Vec3, half_w: f32) -> Option<f32> {
    level
        .blocks
        .iter()
        .filter(|b| overlaps_xz(b, feet, half_w) && b.max.y <= feet.y + 1e-3)
        .map(|b| feet.y - b.max.y)
        .fold(None, |best: Option<f32>, d| Some(best.map_or(d, |x| x.min(d))))
}

fn overlaps_xz(b: &Block, p: Vec3, half_w: f32) -> bool {
    p.x + half_w > b.min.x && p.x - half_w < b.max.x && p.z + half_w > b.min.z && p.z - half_w < b.max.z
}

fn overlaps_box(b: &Block, feet: Vec3, half_w: f32, height: f32) -> bool {
    overlaps_xz(b, feet, half_w) && feet.y + height > b.min.y + 1e-4 && feet.y < b.max.y - 1e-4
}

fn blocked_above(level: &Level, feet: Vec3, half_w: f32, height: f32) -> bool {
    level.blocks.iter().any(|b| overlaps_box(b, feet, half_w, height))
}

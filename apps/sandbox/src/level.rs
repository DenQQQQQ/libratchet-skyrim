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
    /// Its sides take wall jumps (the game marks such walls; ordinary walls don't).
    pub wall_jump: bool,
}

impl Block {
    fn new(center_xz: Vec2, bottom: f32, size: Vec3) -> Block {
        let min = Vec3::new(center_xz.x - size.x * 0.5, bottom, center_xz.y - size.z * 0.5);
        Block { min, max: min + size, wall_jump: false }
    }

    fn wall_jump(self) -> Block {
        Block { wall_jump: true, ..self }
    }
}

/// A wall-jump zone, as the game's level designers place over wall-jump stretches (its
/// "WallJump" volumes): inside, the camera turns to look along `look`, 3° down, and pulls
/// back, and wall jumps keep their line.
#[derive(Clone, Copy, Debug)]
pub struct WallJumpZone {
    pub min: Vec3,
    pub max: Vec3,
    /// Which way the camera looks, on the ground plane.
    pub look: Vec3,
}

/// How far outside a zone's box still counts as in it (the game tests with a 0.5 margin; that
/// it widens the box is our reading).
const ZONE_MARGIN: f32 = 0.5;

#[derive(Resource, Default)]
pub struct Level {
    pub blocks: Vec<Block>,
    pub zones: Vec<WallJumpZone>,
    /// Placed ledges he can grab and hang from (the game's ledge paths).
    pub ledges: Vec<libratchet::Ledge>,
    /// Swingshot targets.
    pub swing_targets: Vec<libratchet::SwingTarget>,
}

/// The level as the sim asks about it: its placed ledges, and rays against its blocks.
pub struct LevelWorld<'a>(pub &'a Level);

impl libratchet::World for LevelWorld<'_> {
    fn ledges(&self, center: Vec3, radius: f32, out: &mut Vec<libratchet::Ledge>) {
        out.extend(self.0.ledges.iter().filter(|l| l.closest(center).0.distance(center) <= radius).copied());
    }

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<libratchet::RayHit> {
        raycast(self.0, from, to).map(|(point, normal, _)| libratchet::RayHit { point, normal })
    }

    fn swing_targets(&self, center: Vec3, radius: f32, out: &mut Vec<libratchet::SwingTarget>) {
        out.extend(self.0.swing_targets.iter().filter(|t| t.position.distance(center) <= radius).copied());
    }
}

impl Level {
    /// The wall-jump zone around `feet`, if any.
    pub fn wall_jump_zone(&self, feet: Vec3) -> Option<&WallJumpZone> {
        let m = Vec3::splat(ZONE_MARGIN);
        self.zones.iter().find(|z| feet.cmpge(z.min - m).all() && feet.cmple(z.max + m).all())
    }
}

/// Where the course pieces are, for respawn and the demo script.
pub mod spots {
    use bevy::prelude::*;
    pub const SPAWN: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    /// Open strip for side-view demos, running along +X.
    pub const DEMO_LANE: Vec3 = Vec3::new(-45.0, 0.0, 100.0);
    /// Top of the glide tower.
    pub const TOWER_TOP: Vec3 = Vec3::new(-30.0, 12.0, 60.0);
    /// Open ground with target dummies for the wrench (they stand along +X from here).
    pub const ARENA: Vec3 = Vec3::new(25.0, 0.0, 30.0);
    /// The middle of the wall-jump shaft's floor: its two walls face each other across Z.
    pub const WALL_SHAFT: Vec3 = Vec3::new(-20.0, 0.0, 20.0);
    /// The ledge course: a gap to fall short of (the catch), and a wall with placed ledges,
    /// along +X from here.
    pub const LEDGES: Vec3 = Vec3::new(40.0, 0.0, -30.0);
    /// The Swingshot course: platforms along +Z with swing targets between them, and a
    /// Hookshot target up over a tall block at the end.
    pub const SWINGS: Vec3 = Vec3::new(70.0, 0.0, 0.0);
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
    level.blocks.push(Block { min: Vec3::new(-FLOOR, -1.0, -FLOOR), max: Vec3::new(FLOOR, 0.0, FLOOR), wall_jump: false });
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

    // Wall-jump shaft: two 12.5 m walls that take wall jumps (teal), 3.4 m apart across Z, with a
    // ledge behind the +Z one, in a wall-jump zone whose camera looks along the shaft from +X.
    // Beside it, two plain walls of the same height and spacing (grey), across X.
    let shaft = spots::WALL_SHAFT;
    const GAP: f32 = 3.4;
    for side in [-1.0, 1.0] {
        let wall = Block::new(Vec2::new(shaft.x, shaft.z + side * (GAP * 0.5 + 0.4)), 0.0, Vec3::new(6.0, 12.5, 0.8));
        add(&mut commands, wall.wall_jump(), Color::srgb(0.15, 0.7, 0.75));
    }
    add(&mut commands, Block::new(Vec2::new(shaft.x, shaft.z + GAP * 0.5 + 2.8), 0.0, Vec3::new(6.0, 12.5, 4.0)), Color::srgb(0.15, 0.6, 0.65));
    for x in [shaft.x - 10.0, shaft.x - 10.0 - GAP - 0.8] {
        add(&mut commands, Block::new(Vec2::new(x, shaft.z), 0.0, Vec3::new(0.8, 12.5, 6.0)), Color::srgb(0.55, 0.55, 0.6));
    }
    // Ledge course (along +X): a 3 m block to jump from across a 3 m gap at a 5 m platform,
    // whose lip he catches when he falls short; then a 5.5 m wall with a placed ledge at 3 m (a
    // yellow shelf) and another along its top, also marked yellow.
    let l = spots::LEDGES;
    add(&mut commands, Block::new(Vec2::new(l.x - 7.5, l.z), 0.0, Vec3::new(3.0, 3.0, 4.0)), Color::srgb(0.6, 0.5, 0.4));
    add(&mut commands, Block::new(Vec2::new(l.x - 1.0, l.z), 0.0, Vec3::new(4.0, 5.0, 4.0)), Color::srgb(0.7, 0.55, 0.4));
    let face = l.x + 6.0;
    add(&mut commands, Block::new(Vec2::new(face + 3.0, l.z), 0.0, Vec3::new(6.0, 5.5, 4.0)), Color::srgb(0.55, 0.45, 0.4));
    add(&mut commands, Block::new(Vec2::new(face - 0.075, l.z), 2.85, Vec3::new(0.15, 0.15, 4.0)), Color::srgb(0.95, 0.8, 0.2));
    // Swingshot course (along +Z): a 2 m platform, three targets 9 m up and 12 m apart (the
    // game picks the farthest target in range, so only the next one may be in range when he
    // looks for it), a 4 m landing platform, and a 6 m block with a Hookshot target 4 m over it.
    // The reel sags him under the cable and he comes off it still swinging, so the block is low
    // enough to clear and deep enough to land on.
    let sw = spots::SWINGS;
    add(&mut commands, Block::new(Vec2::new(sw.x, sw.z - 2.0), 0.0, Vec3::new(4.0, 2.0, 4.0)), Color::srgb(0.45, 0.5, 0.65));
    add(&mut commands, Block::new(Vec2::new(sw.x, sw.z + 41.0), 0.0, Vec3::new(4.0, 4.0, 10.0)), Color::srgb(0.45, 0.5, 0.65));
    add(&mut commands, Block::new(Vec2::new(sw.x, sw.z + 57.0), 0.0, Vec3::new(6.0, 6.0, 10.0)), Color::srgb(0.4, 0.45, 0.6));
    level.zones.push(WallJumpZone {
        min: Vec3::new(shaft.x - 3.0, 0.0, shaft.z - GAP * 0.5),
        max: Vec3::new(shaft.x + 3.0, 12.5, shaft.z + GAP * 0.5),
        look: Vec3::NEG_X,
    });
    for (id, x, y) in [(1, face - 0.15, 3.0), (2, face, 5.5)] {
        level.ledges.push(libratchet::Ledge {
            a: Vec3::new(x, y, l.z - 2.0),
            b: Vec3::new(x, y, l.z + 2.0),
            normal: Vec2::NEG_X,
            id,
            flags: libratchet::LedgeFlags::default(),
        });
    }
    for (i, z) in [6.0, 18.0, 30.0].into_iter().enumerate() {
        level.swing_targets.push(libratchet::SwingTarget {
            position: Vec3::new(sw.x, 9.0, sw.z + z),
            kind: libratchet::SwingKind::Swing,
            axis: Vec2::Y,
            length: 5.0,
            min_range: None,
            max_range: None,
            id: i as u32 + 1,
        });
    }
    level.swing_targets.push(libratchet::SwingTarget {
        position: Vec3::new(sw.x, 10.0, sw.z + 55.0),
        kind: libratchet::SwingKind::Hook,
        axis: Vec2::Y,
        length: -1.0,
        min_range: None,
        max_range: None,
        id: 10,
    });
    // The placed ledges, marked (no collision).
    let marker = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.8, 0.2), emissive: LinearRgba::rgb(0.4, 0.3, 0.05), ..default() });
    for ledge in &level.ledges {
        let along = ledge.b - ledge.a;
        let out = Vec3::new(ledge.normal.x, 0.0, ledge.normal.y);
        let size = along.abs() + Vec3::new(0.06, 0.06, 0.06);
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::from_size(size))),
            MeshMaterial3d(marker.clone()),
            Transform::from_translation((ledge.a + ledge.b) * 0.5 + out * 0.03 + Vec3::Y * 0.03),
        ));
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

/// A wall the character ran into: the way it faces (out of the wall, on the ground plane),
/// whether it takes wall jumps, and whether the game's wall probe would find it (see
/// `PROBE_SIDE`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallHit {
    pub normal: Vec2,
    pub wall_jump: bool,
    pub probed: bool,
}

/// The game finds a wall to stick to with three rays at a time: straight at it and this far
/// to either side (RiftApart.exe), so the wall has to reach that far past him both ways, at
/// his middle.
/// The game's probe reaches 0.25 m to either side at Ratchet's 0.35 m half width; it scales
/// with him.
const PROBE_SIDE_PER_HALF_WIDTH: f32 = 0.25 / 0.35;

/// Result of moving the character's box through the level.
pub struct MoveResult {
    pub feet: Vec3,
    pub grounded: bool,
    pub hit_ceiling: bool,
    pub hit_wall: bool,
    /// The wall it ran into, if any (the last one, when two).
    pub wall: Option<WallHit>,
}

/// Moves a box (feet position, half width, height) by `delta`, one axis at a time, stopping at
/// blocks and stepping up small ledges.
pub fn move_box(level: &Level, feet: Vec3, half_w: f32, height: f32, delta: Vec3, step_height: f32) -> MoveResult {
    const SKIN: f32 = 1e-4;
    let mut p = feet;
    let mut grounded = false;
    let mut hit_ceiling = false;
    let mut hit_wall = false;
    let mut wall = None;

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
            // Already inside it (grown into it, say): moving out isn't blocked.
            if !overlaps_box(b, next, half_w, height) || overlaps_box(b, p, half_w, height) {
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
            let out = -d.signum();
            let side = 2 - axis;
            let middle = p.y + height * 0.5;
            let probe = half_w * PROBE_SIDE_PER_HALF_WIDTH;
            let probed = b.min[side] <= p[side] - probe && b.max[side] >= p[side] + probe && b.min.y <= middle && b.max.y >= middle;
            wall = Some(WallHit { normal: if axis == 0 { Vec2::new(out, 0.0) } else { Vec2::new(0.0, out) }, wall_jump: b.wall_jump, probed });
            next[axis] = if d > 0.0 { b.min[axis] - half_w - SKIN } else { b.max[axis] + half_w + SKIN };
        }
        p = next;
    }

    // Standing exactly on a top surface counts as grounded even without moving down.
    if !grounded && delta.y <= 0.0 {
        grounded = level.blocks.iter().any(|b| overlaps_xz(b, p, half_w) && (p.y - b.max.y).abs() <= 1e-3);
    }

    MoveResult { feet: p, grounded, hit_ceiling, hit_wall, wall }
}

/// Sweeps a flight path (the box from `feet`, at velocity `v`, pulled down at `gravity`) for up
/// to 4 s in 32 steps, as the game's trajectory prediction does: true if the first thing it
/// meets is a floor, false for a wall or nothing.
pub fn landing_ahead(level: &Level, feet: Vec3, v: Vec3, gravity: f32, half_w: f32, height: f32) -> bool {
    const STEPS: usize = 32;
    let dt = 4.0 / STEPS as f32;
    let (mut p, mut v) = (feet, v);
    for _ in 0..STEPS {
        let next = v - Vec3::Y * gravity * dt;
        let moved = move_box(level, p, half_w, height, (v + next) * 0.5 * dt, 0.0);
        if moved.hit_wall {
            return false;
        }
        if moved.grounded {
            return true;
        }
        (p, v) = (moved.feet, next);
    }
    false
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

/// Moves the box out of any block it's inside, the shortest way sideways (after it grows).
pub fn push_out(level: &Level, mut feet: Vec3, half_w: f32, height: f32) -> Vec3 {
    for b in &level.blocks {
        if !overlaps_box(b, feet, half_w, height) {
            continue;
        }
        let ways = [
            (b.max.x + half_w - feet.x, Vec3::X),
            (feet.x + half_w - b.min.x, Vec3::NEG_X),
            (b.max.z + half_w - feet.z, Vec3::Z),
            (feet.z + half_w - b.min.z, Vec3::NEG_Z),
        ];
        if let Some((d, dir)) = ways.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)) {
            feet += dir * (d + 1e-4);
        }
    }
    feet
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

/// The first block a ray from `from` to `to` hits: where, the face's normal, and how far along
/// the ray (0..1). Rays starting inside a block don't hit that block.
pub fn raycast(level: &Level, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3, f32)> {
    let d = to - from;
    let mut best: Option<(Vec3, Vec3, f32)> = None;
    'blocks: for b in &level.blocks {
        let (mut near, mut far) = (0.0f32, best.map_or(1.0, |h| h.2));
        let mut normal = Vec3::ZERO;
        for axis in 0..3 {
            let unit = Vec3::AXES[axis];
            if d[axis].abs() < 1e-9 {
                if from[axis] < b.min[axis] || from[axis] > b.max[axis] {
                    continue 'blocks;
                }
                continue;
            }
            let inv = 1.0 / d[axis];
            let (enter, exit, n) = if inv > 0.0 {
                ((b.min[axis] - from[axis]) * inv, (b.max[axis] - from[axis]) * inv, -unit)
            } else {
                ((b.max[axis] - from[axis]) * inv, (b.min[axis] - from[axis]) * inv, unit)
            };
            if enter > near {
                near = enter;
                normal = n;
            }
            far = far.min(exit);
            if near > far {
                continue 'blocks;
            }
        }
        if normal != Vec3::ZERO {
            best = Some((from + d * near, normal, near));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level() -> Level {
        Level {
            blocks: vec![
                Block { min: Vec3::new(-10.0, -1.0, -10.0), max: Vec3::new(10.0, 0.0, 10.0), wall_jump: false },
                Block { min: Vec3::new(2.0, 0.0, -1.0), max: Vec3::new(3.0, 2.0, 1.0), wall_jump: false },
            ],
            zones: Vec::new(),
            ledges: Vec::new(),
            swing_targets: Vec::new(),
        }
    }

    #[test]
    fn a_box_grown_into_a_block_moves_out_the_short_way_and_isnt_thrown_through() {
        let level = level();
        // Standing against the block's -X face, the box grows from 0.35 to 0.55 m half width.
        let feet = Vec3::new(2.0 - 0.36, 0.0, 0.0);
        let out = push_out(&level, feet, 0.55, 1.8);
        assert!((out.x - (2.0 - 0.55)).abs() < 1e-3 && out.z == 0.0, "pushed to {out:?}");
        // Moving while still inside it doesn't put him on its far side.
        let moved = move_box(&level, feet, 0.55, 1.8, Vec3::new(-0.05, 0.0, 0.0), 0.0);
        assert!(moved.feet.x < 2.0, "thrown to {:?}", moved.feet);
    }

    #[test]
    fn rays_hit_the_nearest_face() {
        let level = level();
        // Down onto the floor.
        let (p, n, t) = raycast(&level, Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -5.0, 0.0)).unwrap();
        assert!((p.y).abs() < 1e-5 && n == Vec3::Y && (t - 0.5).abs() < 1e-5);
        // Along +X into the block's side, past the floor's top.
        let (p, n, _) = raycast(&level, Vec3::new(0.0, 1.0, 0.0), Vec3::new(5.0, 1.0, 0.0)).unwrap();
        assert!((p.x - 2.0).abs() < 1e-5 && n == Vec3::NEG_X);
        // Down onto the block's top.
        let (p, n, _) = raycast(&level, Vec3::new(2.5, 5.0, 0.0), Vec3::new(2.5, 0.5, 0.0)).unwrap();
        assert!((p.y - 2.0).abs() < 1e-5 && n == Vec3::Y);
        // Over it: nothing.
        assert!(raycast(&level, Vec3::new(0.0, 3.0, 0.0), Vec3::new(5.0, 3.0, 0.0)).is_none());
    }
}

//! Ledges (RiftApart.exe): the placed ledge paths he grabs on the way down and hangs from, and
//! the automatic "recovery" catch on any lip he falls just short of.
//!
//! - **Placed ledges** (`World::ledges`): grabbed automatically while coming down, inside the
//!   FromAir window. From the hang a jump goes to another ledge above (or below, with the stick
//!   pointing away or the drop button) when there is one, else up over the lip (the ledge
//!   jump); the drop button, or a jump with the stick pointing away, lets go with a turn.
//! - **Recovery ledges** (found with `World::raycast`): caught while coming down from a jump
//!   (ledges ahead only) and for a moment after walking or running off an edge (all round),
//!   when there is room to hang below and to climb on top. A jump climbs up, the drop button
//!   lets go.
//!
//! The game drives the grabs, the climb, the drop and the ledge-to-ledge moves by their clips'
//! root motion (`LedgeParams`); a move that has to end on a given hang point is bent onto it.

use super::{Event, MoveState, Ratchet, MOVE_STICK};
use crate::tuning::{RecoveryParams, RootPath};
use crate::world::{Ledge, World};
use glam::{Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Presses count this long: the jump on a recovery ledge, and both buttons on a placed one
/// (RiftApart.exe; the lengths are our reading of the input checks).
const RECOVERY_BUFFER: f32 = 0.2;
const PATH_BUFFER: f32 = 0.3;
/// The stick points away from the wall when it's within this many degrees of straight out.
const AWAY_ANGLE: f32 = 45.0;
/// A ledge to move to counts as above when it's at least this far over his body, which is this
/// high above his root while he hangs (the hang clip).
const TRANSFER_UP_MIN: f32 = 1.0;
const HANG_BODY: f32 = 0.65;
/// Ledge heights more than this apart, the smaller height change wins over the nearer ledge.
const TRANSFER_HEIGHT_PREFERENCE: f32 = 0.5;
/// A ledge to move to must be at least this much in front of him (direction · ahead).
const TRANSFER_AHEAD: f32 = 0.075;
/// Grabbing a placed ledge looks from this far above his feet (from his feet in double jumps
/// and air dashes), and doesn't check he faces it this close to the ground.
const FROM_AIR_LIFT: f32 = 1.0;
const FROM_AIR_FACING_SKIP: f32 = 2.2;
/// A recovery grab during an air dash takes only ledges this high above him.
const AIR_DASH_RECOVERY: (f32, f32) = (0.5, 1.0);
/// After walking or running off an edge, he looks for a ledge to catch for this long.
const WALK_OFF_WINDOW: f32 = 0.5;
/// Half the distance between his hands on a ledge (the hang clip's hand locators).
const HAND_HALF_SPAN: f32 = 0.24;

/// The ledge he's on, or moving to: the point on the edge he holds, the way the wall below it
/// faces (out of it), and the placed ledge it's on (none for a caught one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hold {
    pub point: Vec3,
    pub normal: Vec2,
    pub path: Option<Ledge>,
}

impl Hold {
    /// Where his root hangs from it.
    pub fn hang_point(&self, out: f32, down: f32) -> Vec3 {
        self.point + Vec3::new(self.normal.x, 0.0, self.normal.y) * out - Vec3::Y * down
    }
}

/// The clip-driven move he's in onto a ledge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgeMove {
    /// Onto a placed ledge from the air.
    Grab,
    /// Onto a ledge he caught.
    FromFall,
    /// To a placed ledge above or below.
    UpTo,
    DownTo,
}

/// Why he's looking for a ledge to catch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    /// Coming down from a jump, a fall or a glide: ledges ahead only.
    Jump,
    /// Just walked or ran off an edge: all round, but behind him only over a big drop.
    WalkOff,
}

fn facing_from(normal: Vec2) -> f32 {
    (-normal.x).atan2(-normal.y)
}

/// How far along its own motion a clip is at `t` (0..1); a clip without root motion gets there
/// in a tenth of a second.
fn progress(path: &RootPath, t: f32) -> f32 {
    let goal = path.end();
    if goal.length_squared() > 1e-6 { (path.at(t).dot(goal) / goal.length_squared()).clamp(0.0, 1.0) } else { (t / 0.1).min(1.0) }
}

impl Ratchet {
    /// Grabs a ledge in reach while he's coming down: a placed one first, then one to catch.
    pub(crate) fn try_grab_ledge(&mut self, world: &dyn World, position: Vec3, ground_distance: Option<f32>) -> bool {
        let from_feet = matches!(self.state, MoveState::DoubleJump | MoveState::AirDash);
        if let Some(hold) = self.find_path_ledge(world, position, from_feet, ground_distance) {
            self.grab(hold, position, LedgeMove::Grab);
            return true;
        }
        let mode = if self.walk_off_time > 0.0 { Recovery::WalkOff } else { Recovery::Jump };
        let s = self.tuning.scale;
        let window = if self.state == MoveState::AirDash { (AIR_DASH_RECOVERY.0 * s, AIR_DASH_RECOVERY.1 * s) } else { (f32::NEG_INFINITY, f32::INFINITY) };
        if let Some(hold) = self.find_recovery_ledge(world, position, mode, window) {
            self.grab(hold, position, LedgeMove::FromFall);
            return true;
        }
        false
    }

    fn grab(&mut self, hold: Hold, position: Vec3, how: LedgeMove) {
        self.hold = Some(hold);
        self.start_ledge_move(position, how);
        self.velocity = Vec3::ZERO;
        self.extending = false;
        self.walk_off_time = 0.0;
        // Off the air: his air moves are back, as on the ground (ours).
        self.midair_jumps = 0;
        self.air_dash_used = false;
        self.wall_sticks = 0;
        self.chain_origin_y = None;
        self.events.push(Event::LedgeGrabbed { caught: hold.path.is_none() });
    }

    fn start_ledge_move(&mut self, position: Vec3, how: LedgeMove) {
        self.ledge_move = Some(how);
        self.ledge_from = position;
        self.ledge_time = 0.0;
        self.state = MoveState::LedgeHang;
        if let Some(h) = self.hold {
            self.facing_yaw = facing_from(h.normal);
        }
    }

    /// A placed ledge inside the FromAir window: up to `side` past its ends, up to `forward` out
    /// from it, its height within the window over a point FROM_AIR_LIFT above him, facing him
    /// with him facing it (not checked near the ground), and not the one he just left.
    fn find_path_ledge(&self, world: &dyn World, position: Vec3, from_feet: bool, ground_distance: Option<f32>) -> Option<Hold> {
        let w = &self.tuning.ledge.from_air;
        let s = self.tuning.scale;
        let eye = position + Vec3::Y * if from_feet { 0.0 } else { FROM_AIR_LIFT * s };
        let mut ledges = Vec::new();
        world.ledges(eye, w.forward + w.side + w.height_max.max(-w.height_min) + 1.0 * s, &mut ledges);
        let facing = Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos());
        let check_facing = ground_distance.is_none_or(|d| d > FROM_AIR_FACING_SKIP * s);
        let mut best: Option<(f32, Hold)> = None;
        for ledge in ledges {
            if !ledge.flags.grab || self.left_ledge == Some(ledge.id) {
                continue;
            }
            let along = ledge.b - ledge.a;
            let len = along.length();
            if len < 1e-4 {
                continue;
            }
            let t = (eye - ledge.a).dot(along / len);
            if t < -w.side || t > len + w.side {
                continue;
            }
            let point = ledge.a + along / len * t.clamp(0.0, len);
            let to_eye = Vec2::new(eye.x - point.x, eye.z - point.z);
            let out = to_eye.dot(ledge.normal);
            let height = point.y - eye.y;
            if !(0.0..=w.forward).contains(&out) || height < w.height_min || height > w.height_max {
                continue;
            }
            if check_facing && facing.dot(-ledge.normal) <= 0.0 {
                continue;
            }
            let distance = to_eye.length() + height.abs();
            if best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, Hold { point, normal: ledge.normal, path: Some(ledge) }));
            }
        }
        best.map(|(_, h)| h)
    }

    /// The automatic catch (the game's recovery ledge search, done with rays): a lip ahead (all
    /// round after walking off) within reach and the search heights, its top level enough, the
    /// wall below it not sticking out, room to hang below it and to climb onto it, and both
    /// hands on it.
    fn find_recovery_ledge(&self, world: &dyn World, position: Vec3, mode: Recovery, window: (f32, f32)) -> Option<Hold> {
        let p: &RecoveryParams = match mode {
            Recovery::Jump => &self.tuning.ledge.recovery_jump,
            Recovery::WalkOff => &self.tuning.ledge.recovery_run_off,
        };
        let s = self.tuning.scale;
        let facing = Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos());
        let turns: &[f32] = match mode {
            Recovery::Jump => &[0.0, -0.5, 0.5],
            Recovery::WalkOff => &[0.0, -0.5, 0.5, -FRAC_PI_2, FRAC_PI_2, -3.0 * FRAC_PI_4, 3.0 * FRAC_PI_4, PI],
        };
        // The game's search reaches lower, but the catch clip has him take the lip once it's
        // this far over his feet (it then sinks him into the hang), so lower lips wait (ours).
        let catch = self.tuning.ledge.hang_down + self.tuning.ledge.from_fall.end().y;
        let lo = position.y + p.search_min.max(catch).max(window.0);
        let hi = position.y + p.search_max.min(window.1);
        if lo > hi {
            return None;
        }
        let min_up = p.max_slope.to_radians().cos();
        for &turn in turns {
            let (sin, cos) = turn.sin_cos();
            let d = Vec2::new(facing.x * cos + facing.y * sin, -facing.x * sin + facing.y * cos);
            let d3 = Vec3::new(d.x, 0.0, d.y);
            // The top: the first level-enough surface in the height window, going out.
            const STEPS: usize = 6;
            let top = (1..=STEPS).find_map(|i| {
                let x = position + d3 * (p.reach * i as f32 / STEPS as f32);
                let hit = world.raycast(Vec3::new(x.x, hi + 0.05 * s, x.z), Vec3::new(x.x, lo - 0.05 * s, x.z))?;
                (hit.normal.y >= min_up).then_some(hit.point.y)
            });
            let Some(top) = top else { continue };
            // The lip: the wall face just under the top, out from him.
            let from = Vec3::new(position.x, top - 0.05 * s, position.z);
            let Some(wall) = world.raycast(from, from + d3 * (p.reach + 0.2 * s)) else { continue };
            let flat = Vec2::new(wall.normal.x, wall.normal.z);
            if wall.normal.y.abs() > 0.3 || flat.length_squared() < 0.5 || flat.normalize().dot(d) > -0.5 {
                continue;
            }
            let normal = flat.normalize();
            let n3 = Vec3::new(normal.x, 0.0, normal.y);
            let lip = Vec3::new(wall.point.x, top, wall.point.z);
            let behind = (lip - position).dot(Vec3::new(facing.x, 0.0, facing.y)) < 0.0;
            if top - self.last_ground_y > p.max_above_last_ground {
                continue;
            }
            // The wall below doesn't stick out toward him.
            let low = Vec3::new(position.x, top - p.wall_check, position.z);
            if let Some(below) = world.raycast(low, low + d3 * (p.reach + 0.2 * s))
                && below.point.distance(low) < wall.point.distance(from) - 0.3 * s
            {
                continue;
            }
            // Room to hang: nothing within the drop below the wall check, out where he'd hang.
            let column = lip + n3 * self.tuning.ledge.hang_out - Vec3::Y * p.wall_check;
            let drop = if behind { p.min_drop_behind } else { p.min_drop };
            if world.raycast(column, column - Vec3::Y * drop).is_some() {
                continue;
            }
            // Room to climb on top.
            let on_top = lip - n3 * 0.05 * s;
            let clear = [0.2 * s, p.upper_height].iter().all(|&h| world.raycast(on_top + Vec3::Y * h, on_top + Vec3::Y * h - n3 * p.upper_reach).is_none());
            // Both hands on the lip.
            let side = Vec3::new(-normal.y, 0.0, normal.x) * HAND_HALF_SPAN * s;
            let hands = [side, -side].iter().all(|&o| {
                let x = lip - n3 * 0.1 * s + o;
                world.raycast(x + Vec3::Y * 0.3 * s, x - Vec3::Y * 0.3 * s).is_some_and(|h| (h.point.y - top).abs() < 0.1 * s)
            });
            if clear && hands {
                return Some(Hold { point: lip, normal, path: None });
            }
        }
        None
    }

    /// On a ledge: the clip-driven move onto it (bent onto the hang point), then hanging there
    /// until a button.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ledge_hang(&mut self, dt: f32, jump_age: f32, crouch_age: f32, stick: Vec2, world: &dyn World, position: Vec3, grounded: bool) {
        let Some(hold) = self.hold else {
            self.enter_fall();
            return;
        };
        self.ledge_time += dt;
        let target = hold.hang_point(self.tuning.ledge.hang_out, self.tuning.ledge.hang_down);
        if let Some(how) = self.ledge_move {
            let path = self.ledge_path(how).clone();
            let at = self.ledge_from + (target - self.ledge_from) * progress(&path, self.ledge_time);
            self.velocity = (at - position) / dt.max(1e-6);
            if self.ledge_time >= path.duration {
                self.ledge_move = None;
            }
            // The catch takes buttons from its early exit; moves onto a placed ledge once over.
            let ready = match how {
                LedgeMove::Grab | LedgeMove::FromFall => self.ledge_time >= path.exit,
                LedgeMove::UpTo | LedgeMove::DownTo => self.ledge_move.is_none(),
            };
            if !ready {
                return;
            }
        } else {
            // Feet on the ground: he's off the ledge.
            if grounded {
                self.hold = None;
                self.land();
                return;
            }
            self.velocity = (target - position) / dt.max(1e-6);
        }
        self.ledge_buttons(dt, jump_age, crouch_age, stick, world, hold, position);
    }

    fn ledge_path(&self, how: LedgeMove) -> &RootPath {
        let l = &self.tuning.ledge;
        match how {
            LedgeMove::Grab => &l.grab,
            LedgeMove::FromFall => &l.from_fall,
            LedgeMove::UpTo => &l.up_to_ledge,
            LedgeMove::DownTo => &l.down_to_ledge,
        }
    }

    /// The buttons on a ledge.
    #[allow(clippy::too_many_arguments)]
    fn ledge_buttons(&mut self, dt: f32, jump_age: f32, crouch_age: f32, stick: Vec2, world: &dyn World, hold: Hold, position: Vec3) {
        let away = stick.length() >= MOVE_STICK && stick.normalize().dot(hold.normal) >= AWAY_ANGLE.to_radians().cos();
        let Some(path) = hold.path else {
            // A caught ledge: jump climbs up, the drop button lets go.
            if jump_age <= RECOVERY_BUFFER {
                self.jump_press_age = f32::INFINITY;
                self.ledge_from = position;
                self.ledge_time = 0.0;
                self.ledge_move = None;
                self.state = MoveState::LedgeClimb;
                self.events.push(Event::LedgeClimbed);
            } else if crouch_age <= RECOVERY_BUFFER {
                self.crouch_press_age = f32::INFINITY;
                self.let_go(hold, false);
            }
            return;
        };
        let jump = jump_age <= PATH_BUFFER;
        let drop = crouch_age <= PATH_BUFFER;
        if !jump && !drop {
            return;
        }
        self.jump_press_age = f32::INFINITY;
        self.crouch_press_age = f32::INFINITY;
        // To another ledge: below with the stick pointing away or the drop button, else above.
        if path.flags.transfer
            && let Some(next) = self.find_transfer(world, hold, position, away || drop)
        {
            let body = position.y + HANG_BODY * self.tuning.scale;
            let how = if next.point.y - body >= TRANSFER_UP_MIN * self.tuning.scale { LedgeMove::UpTo } else { LedgeMove::DownTo };
            self.left_ledge = Some(path.id);
            self.hold = Some(next);
            self.start_ledge_move(position, how);
            self.events.push(Event::LedgeTransferred);
            return;
        }
        if jump && !away && path.flags.jump {
            self.ledge_jump(dt, hold, position);
        } else if path.flags.drop {
            self.let_go(hold, true);
        }
    }

    /// Another placed ledge to move to: within the FromLedge window, facing the same way, in
    /// front of him, not this one; the nearest, but the smaller height change when the heights
    /// differ by more than half a metre.
    fn find_transfer(&self, world: &dyn World, hold: Hold, position: Vec3, below: bool) -> Option<Hold> {
        let w = &self.tuning.ledge.from_ledge;
        let s = self.tuning.scale;
        let current = hold.path?;
        let mut ledges = Vec::new();
        world.ledges(hold.point, w.forward.max(w.height_max).max(-w.height_min) + w.side + 1.0 * s, &mut ledges);
        let ahead = Vec3::new(-hold.normal.x, 0.0, -hold.normal.y);
        let along = current.along();
        let mut best: Option<(f32, f32, Hold)> = None;
        for ledge in ledges {
            if ledge.id == current.id || !ledge.flags.transfer || ledge.normal.dot(hold.normal) <= 0.0 {
                continue;
            }
            let (point, _) = ledge.closest(hold.point);
            let offset = point - hold.point;
            let rise = offset.y;
            if below != (rise < 0.0) || rise < w.height_min || rise > w.height_max {
                continue;
            }
            if offset.dot(along).abs() > w.side || offset.dot(ahead) > w.forward || (point - position).normalize_or_zero().dot(ahead) <= TRANSFER_AHEAD {
                continue;
            }
            let distance = offset.length();
            let better = match best {
                None => true,
                Some((_, r, _)) if (rise.abs() - r.abs()).abs() > TRANSFER_HEIGHT_PREFERENCE * s => rise.abs() < r.abs(),
                Some((d, _, _)) => distance < d,
            };
            if better {
                best = Some((distance, rise, Hold { point, normal: ledge.normal, path: Some(ledge) }));
            }
        }
        best.map(|(_, _, h)| h)
    }

    /// Up over the lip of a placed ledge: the ledge jump (jump kind 7), straight at the wall at
    /// LedgeJumpSpeeds' run speed for the whole jump.
    fn ledge_jump(&mut self, dt: f32, hold: Hold, position: Vec3) {
        if let Some(path) = hold.path {
            self.left_ledge = Some(path.id);
        }
        self.hold = None;
        self.velocity = Vec3::ZERO;
        self.facing_yaw = facing_from(hold.normal);
        self.start_jump(dt, MoveState::LedgeJump, position);
        self.jump_dir = -hold.normal;
        self.jump_stick = 1.0;
        self.jump_keep_base = true;
        self.events.push(Event::LedgeJumped);
    }

    /// Letting go: of a placed ledge with the turn clip's root motion, of a caught one straight
    /// into a fall.
    fn let_go(&mut self, hold: Hold, turn: bool) {
        if let Some(path) = hold.path {
            self.left_ledge = Some(path.id);
        }
        self.events.push(Event::LedgeDropped);
        if turn {
            self.state = MoveState::LedgeDrop;
            self.ledge_time = 0.0;
            self.ledge_from = hold.hang_point(self.tuning.ledge.hang_out, self.tuning.ledge.hang_down);
            return;
        }
        self.hold = None;
        self.enter_fall();
    }

    /// A clip-driven move off the ledge: his root follows the clip, turned to face the wall.
    /// True once it's over (at the clip's early exit).
    fn ledge_follow(&mut self, dt: f32, position: Vec3, path: &RootPath, normal: Vec2) -> bool {
        self.ledge_time += dt;
        let ahead = Vec3::new(-normal.x, 0.0, -normal.y);
        let left = Vec3::new(ahead.z, 0.0, -ahead.x);
        let at = path.at(self.ledge_time);
        let goal = self.ledge_from + left * at.x + Vec3::Y * at.y + ahead * at.z;
        self.velocity = (goal - position) / dt.max(1e-6);
        self.ledge_time >= path.exit.min(path.duration)
    }

    /// Climbing up onto a caught ledge.
    pub(crate) fn ledge_climb(&mut self, dt: f32, position: Vec3, grounded: bool) {
        let Some(hold) = self.hold else {
            self.enter_fall();
            return;
        };
        let path = self.tuning.ledge.climb_up.clone();
        if self.ledge_follow(dt, position, &path, hold.normal) {
            self.hold = None;
            self.velocity = Vec3::ZERO;
            if grounded {
                self.land();
            } else {
                self.enter_fall();
            }
        }
    }

    /// Letting go of a placed ledge with a turn, then falling, facing away from the wall; on
    /// the ground already, he lands.
    pub(crate) fn ledge_drop(&mut self, dt: f32, position: Vec3, grounded: bool) {
        let Some(hold) = self.hold else {
            self.enter_fall();
            return;
        };
        if grounded && self.ledge_time > 0.0 {
            self.hold = None;
            self.velocity = Vec3::ZERO;
            self.facing_yaw = hold.normal.x.atan2(hold.normal.y);
            self.land();
            return;
        }
        let path = self.tuning.ledge.drop_turn.clone();
        let last = self.velocity;
        if self.ledge_follow(dt, position, &path, hold.normal) {
            self.hold = None;
            self.enter_fall();
            self.velocity = Vec3::new(0.0, last.y.min(0.0), 0.0);
            self.facing_yaw = hold.normal.x.atan2(hold.normal.y);
        }
    }

    /// He walked or ran off an edge: look for a ledge to catch for a moment.
    pub(crate) fn arm_walk_off(&mut self) {
        self.walk_off_time = WALK_OFF_WINDOW;
    }

    /// The ledge he's on or moving onto (the point he holds, and the way its wall faces).
    pub fn ledge(&self) -> Option<(Vec3, Vec2)> {
        self.hold.map(|h| (h.point, h.normal))
    }

    /// The clip-driven move onto a ledge he's in, if any.
    pub fn ledge_move(&self) -> Option<LedgeMove> {
        self.ledge_move
    }
}

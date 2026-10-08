//! The Swingshot (RiftApart.exe): picking a placed target in view, swinging on it, the jump off
//! it, and the Hookshot's reel.
//!
//! - **Targets** (`World::swing_targets`) are picked in a frustum from him, turned like the
//!   camera (HorizFOV × VertFOV full angles, from MinRange out to 60 m), at most MaxRange away,
//!   at most MaxDistBelow below him, and in sight. The game's score rewards targets far off the
//!   view's centre and far away on the level. A target stays usable for GracePeriod after it
//!   last qualified (not once it's out of sight), but ones that qualify now always win.
//! - **The button** must be pressed within 0.2 s and still held: holding it swings.
//! - **The swing** runs in the upright plane along the target's own forward, the cable reeled
//!   from where it hooked toward the target's length. The first swing in is pushed along; then
//!   the stick pumps along the camera's axes, only with the swing. Never above level.
//! - **Letting go** (the button, or jump): the swing jump when it's a jump press, on the first
//!   upswing, or later going up past 30° from straight down; else a fall.
//! - **Hook targets** reel him in along the cable to their length, then drop him.

use super::{Event, Input, MoveState, Ratchet, MOVE_STICK};
use crate::world::{SwingKind, SwingTarget, World};
use glam::{Vec2, Vec3};

/// The button counts when it was pressed this recently (and is still held).
const PRESS_WINDOW: f32 = 0.2;
/// The frustum's apex, this high above his feet: the game puts it at the centre of his bounding
/// sphere. (Assumption: 1 m, the sphere wasn't read.)
const CHEST: f32 = 1.0;
/// A sight line blocked this close to the target still sees it. (Assumption: the game's sight
/// checks weren't read.)
const SIGHT_SLACK: f32 = 0.3;
/// The swing jump steers to sticks past this; its turn never goes past this (degrees).
const JUMP_STEER_STICK: f32 = 0.2;
const JUMP_TURN_LIMIT: f32 = 178.2;
/// The game adds the stick's pump as a velocity once a frame (the push / 30). (Assumption: 60
/// frames a second.)
const PUMP_RATE: f32 = 60.0 / 30.0;

/// The swing in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Swing {
    pub target: SwingTarget,
    /// 0 swinging in, 1 past the target, 2 from the first swing back on. On a Hookshot target:
    /// 1 once the reel is in.
    pub phase: u8,
    /// The cable's length and how fast it's reeling.
    pub length: f32,
    pub reel_rate: f32,
    /// The swing's line (along the target's forward, pointing the way he came in) and the
    /// plane's normal.
    pub axis: Vec3,
    pub normal: Vec3,
    /// The pump's strength, and the pushes ahead and sideways.
    pub strength: f32,
    pub push_ahead: f32,
    pub push_side: f32,
}

/// The mover's turn law (a damped approach with a speed cap), as the game uses it for the cable
/// and the swing jump's turn.
fn approach(value: f32, target: f32, rate: &mut f32, (gain, damp, max): (f32, f32, f32), dt: f32) -> f32 {
    let left = target - value;
    let carry = 0.5 * dt * *rate * (2.0 - (damp * dt / 30.0).exp());
    let step = (carry + (1.0 - (gain * dt).exp()) * left).clamp(-max * dt, max * dt);
    let step = if left.abs() < 1e-5 { left } else { step.clamp(-left.abs(), left.abs()) };
    *rate = step / dt.max(1e-6);
    value + step
}

/// The camera's side (its right on the level) worked out from its forward, for hosts that
/// don't give it: square to the forward, or to `fallback` when the camera looks straight up or
/// down.
fn view_side(forward: Vec3, fallback: Vec3) -> Vec3 {
    let side = forward.cross(Vec3::Y);
    if side.length_squared() > 1e-4 { side.normalize() } else { fallback.cross(Vec3::Y).normalize_or_zero() }
}

impl Ratchet {
    /// The game's target hunt, each tick: which targets qualify, each one's grace period, and
    /// the best-scoring one to fire at.
    pub(crate) fn pick_swing_target(&mut self, world: &dyn World, input: &Input, position: Vec3, dt: f32) {
        let p = &self.tuning.swingshot;
        let s = self.tuning.scale;
        let apex = position + Vec3::Y * CHEST * s;
        let mut targets = Vec::new();
        world.swing_targets(position, p.far.max(p.max_range), &mut targets);
        let facing = Vec3::new(self.facing_yaw.sin(), 0.0, self.facing_yaw.cos());
        let forward = if input.view_forward.length_squared() > 0.25 { input.view_forward.normalize() } else { facing };
        let right = Vec3::new(input.view_right.x, 0.0, input.view_right.y);
        let right = if right.length_squared() > 0.25 { right.normalize() } else { view_side(forward, facing) };
        let up = right.cross(forward).normalize_or_zero();
        let (tan_h, tan_v) = ((p.horiz_fov * 0.5).to_radians().tan(), (p.vert_fov * 0.5).to_radians().tan());
        // The score, from his feet: 1.2 × (1 − cos)/2 of the angle off the camera's forward,
        // plus how far away the target is on the level, over the frustum's 60 m. Both reward
        // targets out to the side and far away. The game also adds a term for the height
        // difference, over a negative scale, so it's always 0.
        let score = |t: &SwingTarget| {
            let d = t.position - position;
            let off = (1.0 - d.normalize_or_zero().dot(forward)) * 0.5;
            1.2 * off.clamp(0.0, 1.0) + (Vec2::new(d.x, d.z).length() / p.far).clamp(0.0, 1.0)
        };
        for seen in &mut self.swing_seen {
            seen.1 += dt;
        }
        // The best one qualifying now; the score has to beat zero.
        let mut best = (0.0, None);
        for t in targets {
            let known = self.swing_seen.iter().position(|(k, _)| k.id == t.id && k.kind == t.kind);
            if world.raycast(apex, t.position).is_some_and(|h| h.point.distance(t.position) > SIGHT_SLACK * s) {
                // Out of sight: dropped at once, grace or not.
                if let Some(i) = known {
                    self.swing_seen.swap_remove(i);
                }
                continue;
            }
            let from_apex = t.position - apex;
            let ahead = from_apex.dot(forward);
            let in_view = ahead > p.near && from_apex.dot(right).abs() <= ahead * tan_h && from_apex.dot(up).abs() <= ahead * tan_v;
            let d = t.position - position;
            let in_range = d.length() <= t.max_range.unwrap_or(p.max_range) && d.length() >= t.min_range.unwrap_or(0.0) && d.y >= -p.max_below;
            if !(in_view && in_range) {
                continue;
            }
            match known {
                Some(i) => self.swing_seen[i] = (t, 0.0),
                None => self.swing_seen.push((t, 0.0)),
            }
            let score = score(&t);
            if score > best.0 {
                best = (score, Some(t));
            }
        }
        self.swing_seen.retain(|(_, age)| *age <= p.grace);
        // Otherwise the best one still in its grace period.
        self.swing_pick = best.1.or_else(|| {
            let grace = self.swing_seen.iter().filter(|(_, age)| *age > 0.0).map(|(t, _)| (score(t), *t));
            grace.fold((0.0, None), |b, (sc, t)| if sc > b.0 { (sc, Some(t)) } else { b }).1
        });
    }

    /// Fires at the picked target if the button was just pressed and is held.
    pub(crate) fn try_swingshot(&mut self, input: &Input, position: Vec3) -> bool {
        if !input.swingshot || self.swing_press_age > PRESS_WINDOW {
            return false;
        }
        let Some(target) = self.swing_pick else { return false };
        self.swing_press_age = f32::INFINITY;
        let to = target.position - position;
        let flat = Vec2::new(to.x, to.z).normalize_or_zero();
        // The swing's line: the target's forward, pointing the way he came in.
        let mut axis = Vec3::new(target.axis.x, 0.0, target.axis.y).normalize_or_zero();
        if axis.length_squared() < 0.5 {
            axis = Vec3::new(flat.x, 0.0, flat.y);
        }
        if axis.dot(Vec3::new(flat.x, 0.0, flat.y)) < 0.0 {
            axis = -axis;
        }
        let speed = self.velocity.length();
        let p = &self.tuning.swingshot;
        self.swingshot = Some(Swing {
            target,
            phase: 0,
            length: to.length(),
            reel_rate: 0.0,
            axis,
            normal: axis.cross(Vec3::Y).normalize_or_zero(),
            strength: (1.4 * speed).min(p.max_speed),
            push_ahead: (1.4 * speed).min(p.max_speed),
            push_side: 0.0,
        });
        self.state = if target.kind == SwingKind::Hook { MoveState::Hook } else { MoveState::Swing };
        self.extending = false;
        self.air_time = 0.0;
        self.hold = None;
        self.events.push(Event::SwingAttached { hook: target.kind == SwingKind::Hook });
        true
    }

    /// One tick of the swing.
    pub(crate) fn swing_tick(&mut self, dt: f32, input: &Input, jump_pressed: bool, position: Vec3, world: &dyn World) {
        let Some(mut sw) = self.swingshot else {
            self.enter_fall();
            return;
        };
        if !self.target_still_there(world, &sw.target) {
            self.swingshot = None;
            self.events.push(Event::SwingReleased);
            self.enter_fall();
            return;
        }
        let p = self.tuning.swingshot.clone();
        let t = sw.target.position;
        let r = position - t;
        let down_angle = (-r.normalize_or_zero().y).clamp(-1.0, 1.0).acos();
        if jump_pressed || !input.swingshot {
            self.swingshot = None;
            let going_up = self.velocity.y >= 0.0;
            let jump = jump_pressed || sw.phase == 1 || (sw.phase == 2 && going_up && down_angle >= p.release_angle.to_radians());
            self.events.push(Event::SwingReleased);
            if jump {
                self.swing_jump(dt, position);
            } else {
                self.enter_fall();
            }
            return;
        }
        let mut v = self.velocity;
        // The game's mover adds gravity after the swing zeroes the fall speed each frame.
        v.y -= p.gravity * dt;
        // In the plane: no speed across it, and any drift out of it dies away.
        let off = r.dot(sw.normal);
        v -= sw.normal * v.dot(sw.normal);
        v += sw.normal * (off * ((-p.plane_pull * dt).exp() - 1.0) / dt.max(1e-6));
        let across = sw.normal * v.dot(sw.normal);
        let in_plane = v - across;
        if in_plane.length() > p.max_speed {
            v = across + in_plane.normalize() * p.max_speed;
        }
        let below = position.y < t.y;
        // The first swing in: pushed along the swing.
        if sw.phase == 0 && below && v.dot(sw.axis) < p.entry_speed {
            v += sw.axis * p.entry_push * dt;
        }
        // Pumping with the stick, along the camera's axes, only with the swing's travel.
        sw.strength = p.pump + (sw.strength - p.pump) * (-p.pump_settle * dt).exp();
        if below {
            let forward = if input.view_forward.length_squared() > 0.25 { input.view_forward.normalize() } else { sw.axis };
            let side = Vec3::new(input.view_right.x, 0.0, input.view_right.y).normalize_or_zero();
            let side = if side == Vec3::ZERO { view_side(forward, sw.axis) } else { side };
            let stick = input.move_dir.clamp_length_max(1.0);
            // The stick here is in the world; read it against the camera's axes.
            let flat_forward = Vec2::new(forward.x, forward.z).normalize_or_zero();
            let flat_side = Vec2::new(side.x, side.z);
            let (stick_y, stick_x) = (stick.dot(flat_forward), stick.dot(flat_side));
            for (amount, deadzone, axis, push) in [(stick_y, p.pump_deadzone.0, forward, &mut sw.push_ahead), (stick_x, p.pump_deadzone.1, side, &mut sw.push_side)] {
                let with_travel = amount.abs() > deadzone && v.dot(axis) * amount > 0.0;
                let goal = if with_travel { sw.strength * amount } else { 0.0 };
                *push += (goal - *push).clamp(-p.pump_follow * dt, p.pump_follow * dt);
                v += axis * *push * PUMP_RATE * dt;
            }
        }
        // The cable reels toward the target's length.
        let mut rate = sw.reel_rate;
        sw.length = approach(sw.length, sw.target.length, &mut rate, p.reel, dt);
        sw.reel_rate = rate;
        // Never above level: past `top_from`, the speed up the arc is held under a cap that
        // closes at level.
        let radial = r.normalize_or_zero();
        let tangent_up = (Vec3::Y - radial * radial.y).normalize_or_zero();
        let up_speed = v.dot(tangent_up);
        if down_angle > p.top_from.to_radians() && v.y > 0.0 && up_speed > 0.0 {
            let cap = (p.top_gain * sw.length * (std::f32::consts::FRAC_PI_2 - down_angle).max(0.0)).sqrt();
            if up_speed > cap {
                let new = if down_angle <= p.top_hard.to_radians() { (up_speed - p.top_ease * dt).max(cap) } else { cap };
                v += tangent_up * (new - up_speed);
            }
        }
        // On the cable.
        let next = t + (position + v * dt - t).normalize_or_zero() * sw.length;
        self.velocity = (next - position) / dt.max(1e-6);
        // Phases: past the target, then the first swing back.
        if sw.phase == 0 && (position - t).dot(sw.axis) > 0.0 {
            sw.phase = 1;
        } else if sw.phase == 1 && v.dot(sw.axis) < 0.0 {
            sw.phase = 2;
        }
        // He faces the way he came in, along the swing.
        self.facing_yaw = sw.axis.x.atan2(sw.axis.z);
        self.swingshot = Some(sw);
    }

    /// The jump off a swing (jump kind 0x30): 2 m up whatever the swing's speed, ahead at least
    /// `jump_min_speed` (more if he was going faster), toward the stick or where he faces.
    fn swing_jump(&mut self, dt: f32, position: Vec3) {
        let flat = Vec2::new(self.velocity.x, self.velocity.z);
        let dir = if self.stick.length() >= MOVE_STICK { self.stick.normalize() } else { Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos()) };
        self.swing_jump_speed = flat.length().max(self.tuning.swingshot.jump_min_speed);
        self.swing_jump_turn = 0.0;
        self.velocity = Vec3::ZERO;
        self.start_jump(dt, MoveState::SwingJump, position);
        self.jump_dir = dir;
        self.facing_yaw = dir.x.atan2(dir.y);
        self.events.push(Event::SwingJumped);
    }

    /// The swing jump's horizontal: its speed held, its direction turned toward the stick by the
    /// mover's turn law.
    pub(crate) fn swing_jump_air(&mut self, dt: f32, stick: Vec2) {
        let turn = self.tuning.swingshot.jump_turn;
        if stick.length() >= JUMP_STEER_STICK {
            let want = stick.normalize();
            let angle = self.jump_dir.angle_to(want).clamp(-JUMP_TURN_LIMIT.to_radians(), JUMP_TURN_LIMIT.to_radians());
            let mut rate = self.swing_jump_turn;
            let turned = approach(0.0, angle, &mut rate, (turn.0, turn.1, turn.2.to_radians()), dt);
            self.swing_jump_turn = rate;
            self.jump_dir = Vec2::from_angle(turned).rotate(self.jump_dir);
        } else {
            self.swing_jump_turn = 0.0;
        }
        let v = self.jump_dir * self.swing_jump_speed;
        self.velocity.x = v.x;
        self.velocity.z = v.y;
    }

    /// One tick of the Hookshot: reeled along the cable to the target's length (its hand ahead
    /// of him on the cable), speeding up and slowing to stop there; sideways speed drags off and
    /// sags. Arrived (the tick after the reel gets there), or with the button let go, he drops.
    pub(crate) fn hook_tick(&mut self, dt: f32, input: &Input, position: Vec3, world: &dyn World) {
        let Some(mut sw) = self.swingshot else {
            self.enter_fall();
            return;
        };
        if !self.target_still_there(world, &sw.target) {
            self.swingshot = None;
            self.events.push(Event::SwingReleased);
            self.enter_fall();
            return;
        }
        let p = self.tuning.swingshot.clone();
        let t = sw.target.position;
        let dir = (t - position).normalize_or_zero();
        let d = (t - (position + dir * p.hook_hand)).length();
        let goal = if sw.target.length > 0.0 { sw.target.length } else { p.hook_length };
        // Arrival goes by the reel rather than the measured distance, which the sag moves off
        // the length again each tick by an amount that depends on the tick rate.
        let arrived = sw.phase == 1;
        if !input.swingshot || arrived {
            self.swingshot = None;
            if arrived {
                self.events.push(Event::HookArrived);
            } else {
                self.events.push(Event::SwingReleased);
            }
            self.enter_fall();
            return;
        }
        // Along the cable: a trapezoid toward the length, stopping on it.
        let left = d - goal;
        let speed = -sw.reel_rate;
        let brake = (2.0 * p.hook_decel * left.abs()).sqrt();
        let speed = (speed + p.hook_accel * dt).min(p.hook_max_speed).min(brake);
        let new_d = if speed * dt >= left { goal } else { d - speed * dt };
        sw.reel_rate = -speed;
        if new_d <= goal {
            sw.phase = 1;
        }
        // Across it: drag, and the pull down across the cable.
        let mut side = self.velocity - dir * self.velocity.dot(dir);
        let drag = (p.hook_drag * side.length_squared() * dt).min(side.length());
        side -= side.normalize_or_zero() * drag;
        let g = Vec3::new(0.0, -p.hook_gravity, 0.0);
        side += (g - dir * g.dot(dir)) * dt;
        let next = t - dir * (new_d + p.hook_hand) + side * dt;
        self.velocity = (next - position) / dt.max(1e-6);
        if Vec2::new(dir.x, dir.z).length() > 0.1 {
            self.facing_yaw = dir.x.atan2(dir.z);
        }
        self.swingshot = Some(sw);
    }

    /// The level still offers the target he's on (the game lets go once it's gone or switched
    /// off).
    fn target_still_there(&self, world: &dyn World, target: &SwingTarget) -> bool {
        let mut near = Vec::new();
        world.swing_targets(target.position, 0.5 * self.tuning.scale, &mut near);
        near.iter().any(|t| t.id == target.id && t.kind == target.kind)
    }

    /// The Swingshot target he'd fire at now, if any.
    pub fn swing_target(&self) -> Option<Vec3> {
        self.swing_pick.map(|t| t.position)
    }

    /// The target he's swinging on or reeling to, and the cable's length.
    pub fn swinging_on(&self) -> Option<(Vec3, f32)> {
        self.swingshot.map(|s| (s.target.position, s.length))
    }

    /// The swing's phase: 0 swinging in, 1 past the target, 2 from the first swing back on.
    pub fn swing_phase(&self) -> Option<u8> {
        self.swingshot.map(|s| s.phase)
    }

    /// The swing's line (level, unit, pointing the way he came in), while he swings: the game
    /// turns its camera to look along it.
    pub fn swing_axis(&self) -> Option<Vec3> {
        self.swingshot.filter(|s| s.target.kind == SwingKind::Swing).map(|s| s.axis)
    }
}

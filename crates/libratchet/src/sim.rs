//! The movement state machine.
//!
//! The host engine owns collision. Each tick it reports contacts and position, calls
//! [`Ratchet::step`], then moves its character by the returned velocity with its own
//! collision (Bevy test scene: a box sweep; Skyrim: the Havok character controller).
//!
//! Coordinates: Y up, metres. Facing yaw is `atan2(x, z)`, so yaw 0 faces +Z.

use crate::tuning::{DashParams, JumpParams, SpeedParams, SwingParams, Tuning};
use crate::world::{NoWorld, World};
use glam::{Vec2, Vec3};

mod ledge;
mod swing;
pub use ledge::LedgeMove;

/// What the player is asking for this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    /// Desired horizontal direction in world space as (x, z), length 0..=1.
    /// The host makes it camera-relative.
    pub move_dir: Vec2,
    /// Jump button is down.
    pub jump: bool,
    /// Dash button is down.
    pub dash: bool,
    /// Melee (wrench) button is down.
    pub melee: bool,
    /// Strafing: the direction (x, z) to face, as while firing a gun (the game strafes while a
    /// gun fires) or aiming.
    pub aim: Option<Vec2>,
    /// Aim mode (the aim button held) on top of `aim`: the slower aim speeds and the aim-mode
    /// jumps. Without it, strafing keeps the normal speeds (firing from the hip).
    pub aim_mode: bool,
    /// Crouch (the game's ledge drop button) is down: lets go of a ledge.
    pub crouch: bool,
    /// The Swingshot button is down: holding it swings on a target in view.
    pub swingshot: bool,
    /// Where the camera looks (unit, or zero without a camera): Swingshot targets are picked
    /// in its view, and a swing pumps along it.
    pub view_forward: Vec3,
    /// The camera's right on the ground plane (x, z), or zero without a camera: the game picks a
    /// wall jump's side by it (its clips, and which way he turns off the wall), and the Swingshot
    /// frames its view and pumps sideways by it (worked out from `view_forward` when zero).
    pub view_right: Vec2,
}

/// What the host's collision found on the last move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub grounded: bool,
    /// Distance from the feet down to the ground, if the host probed for it.
    pub ground_distance: Option<f32>,
    /// The last move bumped into something above.
    pub hit_ceiling: bool,
    /// The wall the last move ran into, if any.
    pub wall: Option<WallContact>,
    /// He's in one of the level's wall-jump zones (the game's "WallJump" volumes, placed over
    /// wall-jump stretches): there a wall jump keeps its line once NoInputTime is over.
    pub wall_jump_zone: bool,
    /// Sweeping his flight path from here (his velocity, and `Ratchet::gravity` down), the
    /// first thing it meets is a floor, not a wall: the game predicts this to end a wall jump in
    /// a fall outside wall-jump zones. Only needed during wall jumps.
    pub landing_ahead: bool,
}

/// A wall the character ran into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallContact {
    /// Out of the wall, on the ground plane (x, z), unit length.
    pub normal: Vec2,
    /// The wall takes wall jumps (the game marks such walls; ordinary ones don't).
    pub sticky: bool,
}

impl Default for Contact {
    fn default() -> Self {
        Contact { grounded: true, ground_distance: Some(0.0), hit_ceiling: false, wall: None, wall_jump_zone: false, landing_ahead: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveState {
    Ground,
    Jump,
    DoubleJump,
    Fall,
    Glide,
    Dash,
    AirDash,
    /// A wrench swing on the ground (see [`Ratchet::melee_swing`] for which one).
    Melee,
    /// Hyper-Strike in the air: the hop, then the slam down.
    Smash,
    /// On the ground right after a Hyper-Strike lands.
    SmashLand,
    /// Stuck to a wall that takes wall jumps: holding still, then sliding down it.
    WallStick,
    /// The jump off a wall.
    WallJump,
    /// Hanging from a ledge, or moving onto one (see [`Ratchet::ledge_move`]).
    LedgeHang,
    /// Climbing up onto a ledge he caught.
    LedgeClimb,
    /// Letting go of a placed ledge, turning away from it.
    LedgeDrop,
    /// The jump up over a placed ledge's lip.
    LedgeJump,
    /// Swinging on a Swingshot target.
    Swing,
    /// Reeled in to a Hookshot target.
    Hook,
    /// The jump off a swing.
    SwingJump,
}

impl MoveState {
    pub fn name(self) -> &'static str {
        match self {
            MoveState::Ground => "ground",
            MoveState::Jump => "jump",
            MoveState::DoubleJump => "double jump",
            MoveState::Fall => "fall",
            MoveState::Glide => "glide",
            MoveState::Dash => "dash",
            MoveState::AirDash => "air dash",
            MoveState::Melee => "melee",
            MoveState::Smash => "hyper-strike",
            MoveState::SmashLand => "hyper-strike landing",
            MoveState::WallStick => "wall stick",
            MoveState::WallJump => "wall jump",
            MoveState::LedgeHang => "ledge hang",
            MoveState::LedgeClimb => "ledge climb",
            MoveState::LedgeDrop => "ledge drop",
            MoveState::LedgeJump => "ledge jump",
            MoveState::Swing => "swing",
            MoveState::Hook => "hookshot",
            MoveState::SwingJump => "swing jump",
        }
    }
}

/// Things that happened this tick, for animation and sound hooks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Jumped,
    DoubleJumped,
    /// Stuck to a wall that takes wall jumps.
    WallStuck,
    /// Jumped off a wall.
    WallJumped,
    LeftGround,
    GlideStarted,
    GlideEnded,
    Dashed,
    AirDashed,
    Landed { fall_speed: f32 },
    /// A combo swing started (0 = first).
    MeleeSwing { index: usize },
    /// The swing's wrench connects now: the host checks for hits.
    MeleeHit { index: usize },
    SmashStarted,
    /// The Hyper-Strike hit the ground: the host applies the slam around the character.
    SmashImpact { fall_speed: f32 },
    /// Grabbed a ledge (`caught`: one found on the way down, not a placed one).
    LedgeGrabbed { caught: bool },
    /// Climbing up a caught ledge.
    LedgeClimbed,
    /// Let go of a ledge.
    LedgeDropped,
    /// Moving to another placed ledge.
    LedgeTransferred,
    /// Jumped up over a placed ledge.
    LedgeJumped,
    /// Fired the Swingshot at a target (`hook`: a Hookshot one).
    SwingAttached { hook: bool },
    /// Let go of the swing or the hook.
    SwingReleased,
    /// Jumped off the swing.
    SwingJumped,
    /// Reeled all the way in to a Hookshot target.
    HookArrived,
}

/// Small downward speed on the ground so the host's collision keeps reporting contact.
const STICK_DOWN_SPEED: f32 = 2.0;
/// A double jump started above its `max_total_jump` still gets this much height
/// (RiftApart.exe).
const OVER_CAP_DOUBLE_JUMP: f32 = 0.5;
/// When a glide starts, its fall-speed limit becomes the current fall speed clamped to this
/// range and stays for the whole glide (RiftApart.exe).
const GLIDE_ENTRY_FALL_MIN: f32 = 4.0;
const GLIDE_ENTRY_FALL_MAX: f32 = 12.0;
/// The stick counts as moving past this (RiftApart.exe: the run transition out of a swing).
const MOVE_STICK: f32 = 0.07;
/// The game's ground turn (RiftApart.exe's mover): (gain, damp, cap in rad/s) while moving
/// (running or strafing, 1500°/s) and standing (aiming or firing, 2500°/s).
const MOVING_TURN: (f32, f32, f32) = (-4.0, -25.0, 26.18);
const STANDING_TURN: (f32, f32, f32) = (-15.0, -8.0, 43.63);
/// Closer than this (radians), the turn snaps onto its target (RiftApart.exe).
const TURN_SNAP: f32 = 0.005;
/// A jump coming down after its NoFallTime only becomes a fall more than this far above the
/// ground (RiftApart.exe).
const NO_FALL_GROUND: f32 = 0.5;
/// A dash on the ground can be jumped out of from this long after it starts (RiftApart.exe).
const DASH_JUMP_AFTER: f32 = 0.1;
/// Stuck to a wall, he's pressed into it at this speed so the host keeps reporting the wall
/// (ours; the wall stops him).
const WALL_PUSH: f32 = 0.5;
/// After a wall jump, no wall within this many metres of the jump-off point (a box around it,
/// along the wall) can be stuck to for this long, the shaft's opposite wall included; landing
/// clears it (RiftApart.exe).
const WALL_BLOCK_REACH: f32 = 7.5;
const WALL_BLOCK_TIME: f32 = 0.5;
/// A wall jump's momentum weights once NoInputTime is over, outside the level's wall-jump
/// zones (fixed in the exe). In a zone it takes its config's, which keep its line.
const WALL_JUMP_MOMENTUM: (f32, f32) = (0.0, 1.4);
/// A jump press counts this long on a wall, and a plain wall takes it once he's been on it this
/// long; a wall-jump wall at once (RiftApart.exe).
const WALL_JUMP_BUFFER: f32 = 0.15;
const PLAIN_WALL_JUMP_DELAY: f32 = 0.15;
/// Pushing the stick into a plain wall this much (its direction against the wall's normal)
/// jumps up it instead of off it (RiftApart.exe).
const WALL_UP_STICK: f32 = -0.25;

#[derive(Clone, Debug)]
pub struct Ratchet {
    tuning: Tuning,
    state: MoveState,
    velocity: Vec3,
    facing_yaw: f32,
    /// Seconds since the current jump started or the character left the ground.
    air_time: f32,
    /// The current jump still grows while the button stays down.
    extending: bool,
    jump_origin_y: f32,
    /// Take-off height of the first jump in the chain, for `max_total_jump`.
    chain_origin_y: Option<f32>,
    /// The current jump's tap/held heights (after any `max_total_jump` cap) and its gravity.
    jump_height_min: f32,
    jump_height_max: f32,
    jump_gravity: f32,
    /// Fall-speed limit of the current glide, fixed when it starts.
    glide_terminal: f32,
    midair_jumps: u32,
    prev_jump: bool,
    prev_dash: bool,
    dash_dir: Vec2,
    dash_speed: f32,
    dash_travelled: f32,
    dash_braking: bool,
    /// Seconds into the dash, and whether he was strafing (aiming or firing) when it started.
    dash_time: f32,
    dash_strafe: bool,
    /// A jump pressed early in the dash, waiting for it to allow one.
    dash_jump_queued: bool,
    air_dash_used: bool,
    /// The jump's take-off (RiftApart.exe): the speed he had, the stick's push and direction
    /// (ahead without input), whether this is its first tick, whether the stick has been left
    /// alone since, and the velocity it holds while it can't be steered yet.
    jump_takeoff_speed: f32,
    jump_stick: f32,
    jump_dir: Vec2,
    jump_first: bool,
    jump_carry: bool,
    jump_held: Vec2,
    /// This jump's first tick goes at the jump speeds, not the take-off speed (wall jumps).
    jump_keep_base: bool,
    /// The wall he's stuck to or last jumped off (out of it), and how long he's been on it.
    wall_normal: Vec2,
    wall_time: f32,
    /// The last wall jump's take-off point and the time left of its block on nearby walls.
    wall_block: Option<(Vec3, f32)>,
    /// Whether the game's right wall clips fit (see `wall_right`).
    wall_right: bool,
    /// The wall he's on takes wall jumps; walls stuck to this air time; whether his double
    /// jump was still unused when he stuck; and the time left on an unused jump press.
    wall_marked: bool,
    wall_sticks: u32,
    wall_up_ok: bool,
    jump_press_age: f32,
    /// The crouch button: when it was last pressed (unused), and whether it was down.
    crouch_press_age: f32,
    prev_crouch: bool,
    /// Ledges: the one he's on or moving onto, the clip-driven move onto it, where the current
    /// clip-driven move started and how long it's run, the placed ledge he last left (no
    /// grabbing it again until he lands), the time left to catch the edge he walked off, and
    /// the height of the last ground he stood on.
    hold: Option<ledge::Hold>,
    ledge_move: Option<LedgeMove>,
    ledge_from: Vec3,
    ledge_time: f32,
    left_ledge: Option<u32>,
    walk_off_time: f32,
    last_ground_y: f32,
    /// The Swingshot: the swing in progress; the targets seen lately (with how long since each
    /// last qualified) and the one it would fire at; the button's press; and the swing jump's
    /// speed and turn.
    swingshot: Option<swing::Swing>,
    swing_seen: Vec<(crate::world::SwingTarget, f32)>,
    swing_pick: Option<crate::world::SwingTarget>,
    swing_press_age: f32,
    prev_swingshot: bool,
    swing_jump_speed: f32,
    swing_jump_turn: f32,
    /// In a wall-jump zone (`Contact::wall_jump_zone`) this tick.
    in_wall_jump_zone: bool,
    prev_melee: bool,
    /// Seconds since the last melee press nothing used yet.
    since_melee_press: f32,
    /// After a swing: the one a press starts while the combo is alive, the seconds it stays
    /// alive, and the seconds presses are ignored after the combo's last swing.
    combo_next: usize,
    combo_alive: f32,
    combo_rest: f32,
    /// Current combo swing, seconds into it (or into the Hyper-Strike / its landing), a press
    /// waiting for the next swing, and whether this swing's hit already happened.
    swing: usize,
    swing_time: f32,
    swing_queued: bool,
    swing_hit: bool,
    smash_descending: bool,
    /// Strafing this tick (facing this direction), in aim mode, and whether the current jump
    /// started in aim mode.
    aiming: Option<Vec2>,
    aim_mode: bool,
    jump_aimed: bool,
    /// This tick's stick, and the turn's angular speed (rad/s), which the next tick carries on.
    stick: Vec2,
    view_right: Vec2,
    turn_omega: f32,
    events: Vec<Event>,
    /// Vertical speed to move by this tick, when it differs from the end-of-tick velocity.
    move_vy: Option<(f32, f32)>,
}

impl Ratchet {
    pub fn new(tuning: Tuning) -> Self {
        Ratchet {
            tuning,
            state: MoveState::Ground,
            velocity: Vec3::ZERO,
            facing_yaw: 0.0,
            air_time: 0.0,
            extending: false,
            jump_origin_y: 0.0,
            chain_origin_y: None,
            jump_height_min: 0.0,
            jump_height_max: 0.0,
            jump_gravity: 0.0,
            glide_terminal: 0.0,
            midair_jumps: 0,
            prev_jump: false,
            prev_dash: false,
            dash_dir: Vec2::ZERO,
            dash_speed: 0.0,
            dash_travelled: 0.0,
            dash_braking: false,
            dash_time: 0.0,
            dash_strafe: false,
            dash_jump_queued: false,
            air_dash_used: false,
            jump_takeoff_speed: 0.0,
            jump_stick: 0.0,
            jump_dir: Vec2::ZERO,
            jump_first: false,
            jump_carry: false,
            jump_held: Vec2::ZERO,
            jump_keep_base: false,
            wall_normal: Vec2::ZERO,
            wall_time: 0.0,
            wall_block: None,
            wall_right: false,
            wall_marked: false,
            wall_sticks: 0,
            wall_up_ok: false,
            jump_press_age: f32::INFINITY,
            crouch_press_age: f32::INFINITY,
            prev_crouch: false,
            hold: None,
            ledge_move: None,
            ledge_from: Vec3::ZERO,
            ledge_time: 0.0,
            left_ledge: None,
            walk_off_time: 0.0,
            last_ground_y: 0.0,
            swingshot: None,
            swing_seen: Vec::new(),
            swing_pick: None,
            swing_press_age: f32::INFINITY,
            prev_swingshot: false,
            swing_jump_speed: 0.0,
            swing_jump_turn: 0.0,
            in_wall_jump_zone: false,
            prev_melee: false,
            since_melee_press: f32::INFINITY,
            combo_next: 0,
            combo_alive: 0.0,
            combo_rest: 0.0,
            swing: 0,
            swing_time: 0.0,
            swing_queued: false,
            swing_hit: false,
            smash_descending: false,
            aiming: None,
            aim_mode: false,
            jump_aimed: false,
            stick: Vec2::ZERO,
            view_right: Vec2::ZERO,
            turn_omega: 0.0,
            events: Vec::new(),
            move_vy: None,
        }
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }

    /// Swaps tuning without resetting state (for live reload).
    pub fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning = tuning;
    }

    pub fn state(&self) -> MoveState {
        self.state
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// Overrides velocity, e.g. after the host resolved a collision.
    pub fn set_velocity(&mut self, v: Vec3) {
        self.velocity = v;
    }

    pub fn facing_yaw(&self) -> f32 {
        self.facing_yaw
    }

    pub fn set_facing_yaw(&mut self, yaw: f32) {
        self.facing_yaw = yaw;
    }

    pub fn facing_dir(&self) -> Vec3 {
        Vec3::new(self.facing_yaw.sin(), 0.0, self.facing_yaw.cos())
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn air_time(&self) -> f32 {
        self.air_time
    }

    pub fn midair_jumps_used(&self) -> u32 {
        self.midair_jumps
    }

    /// The combo swing in progress (0 = first), while in [`MoveState::Melee`].
    pub fn melee_swing(&self) -> Option<usize> {
        (self.state == MoveState::Melee).then_some(self.swing)
    }

    /// Whether the current (or last) dash started while strafing: he faces the aim through it.
    pub fn dash_strafing(&self) -> bool {
        self.dash_strafe
    }

    /// Where the current (or last) dash goes, on the ground plane (x, z).
    pub fn dash_dir(&self) -> Vec2 {
        self.dash_dir
    }

    /// Seconds into the current swing, Hyper-Strike or Hyper-Strike landing.
    pub fn melee_time(&self) -> f32 {
        self.swing_time
    }

    /// Advances one tick and returns the velocity the host should move by. Without a level to
    /// ask about ledges; see [`Ratchet::step_in`].
    pub fn step(&mut self, dt: f32, input: &Input, contact: &Contact, position: Vec3) -> Vec3 {
        self.step_in(dt, input, contact, position, &NoWorld)
    }

    /// Advances one tick in `world`, which it asks for ledges, and returns the velocity the host
    /// should move by.
    pub fn step_in(&mut self, dt: f32, input: &Input, contact: &Contact, position: Vec3, world: &dyn World) -> Vec3 {
        self.events.clear();
        // A tick that takes no time (a paused frame) changes nothing, and several rules divide
        // by it. Its presses still count on the next tick.
        if !(dt > 0.0 && dt.is_finite()) {
            return self.velocity;
        }
        let pressed = input.jump && !self.prev_jump;
        self.prev_jump = input.jump;
        self.jump_press_age = if pressed { 0.0 } else { self.jump_press_age + dt };
        let crouch_pressed = input.crouch && !self.prev_crouch;
        self.prev_crouch = input.crouch;
        self.crouch_press_age = if crouch_pressed { 0.0 } else { self.crouch_press_age + dt };
        self.walk_off_time = (self.walk_off_time - dt).max(0.0);
        let swing_pressed = input.swingshot && !self.prev_swingshot;
        self.prev_swingshot = input.swingshot;
        self.swing_press_age = if swing_pressed { 0.0 } else { self.swing_press_age + dt };
        let dash_pressed = input.dash && !self.prev_dash;
        self.prev_dash = input.dash;
        let melee_pressed = input.melee && !self.prev_melee;
        self.prev_melee = input.melee;
        self.since_melee_press = if melee_pressed { 0.0 } else { self.since_melee_press + dt };
        self.combo_alive = (self.combo_alive - dt).max(0.0);
        self.combo_rest = (self.combo_rest - dt).max(0.0);
        let stick = input.move_dir.clamp_length_max(1.0);
        self.aiming = input.aim.filter(|a| a.length_squared() > 1e-6).map(Vec2::normalize);
        self.aim_mode = input.aim_mode && self.aiming.is_some();
        self.stick = stick;
        self.view_right = input.view_right;
        self.in_wall_jump_zone = contact.wall_jump_zone;
        if let Some((_, left)) = &mut self.wall_block {
            *left -= dt;
        }

        // Contact changes first, so this tick runs in the right state.
        if matches!(self.state, MoveState::Ground | MoveState::Dash | MoveState::Melee | MoveState::SmashLand) {
            if !contact.grounded {
                // Dashing (or lunging) off a ledge keeps its speed and falls. Walking or running
                // off one, he can catch it for a moment (RiftApart.exe).
                if self.state == MoveState::Ground {
                    self.arm_walk_off();
                }
                // The ground states press him down to keep contact; the fall starts from the
                // vertical speed he really had, none.
                self.velocity.y = 0.0;
                self.enter_fall();
                self.events.push(Event::LeftGround);
            }
        } else if contact.grounded
            && self.velocity.y <= 0.0
            && !matches!(self.state, MoveState::LedgeHang | MoveState::LedgeClimb | MoveState::LedgeDrop | MoveState::Swing | MoveState::Hook)
        {
            if self.state == MoveState::Smash {
                self.smash_land();
            } else {
                self.land();
            }
        }
        if contact.hit_ceiling && self.velocity.y > 0.0 {
            self.velocity.y = 0.0;
            self.extending = false;
        }

        // The Swingshot: the target in view (the game looks all the time), and firing at it from
        // the ground, the air or a wall.
        self.pick_swing_target(world, input, position, dt);
        let free = matches!(
            self.state,
            MoveState::Ground
                | MoveState::Jump
                | MoveState::DoubleJump
                | MoveState::Fall
                | MoveState::Glide
                | MoveState::AirDash
                | MoveState::WallStick
                | MoveState::WallJump
                | MoveState::LedgeJump
                | MoveState::SwingJump
        );
        if free && self.state == MoveState::Glide && input.swingshot && self.swing_press_age <= 0.2 && self.swing_pick.is_some() {
            self.events.push(Event::GlideEnded);
        }
        if free {
            self.try_swingshot(input, position);
        }

        match self.state {
            MoveState::Ground => {
                self.last_ground_y = position.y;
                let speed = self.ground_speed_for(stick);
                let (accel, decel) = (self.tuning.ground_accel, self.tuning.ground_decel);
                self.steer(dt, stick, speed, accel, decel);
                if pressed {
                    self.start_jump(dt, MoveState::Jump, position);
                } else if self.melee_wanted() && self.start_swing(self.combo_swing(), stick) {
                    self.swing(dt, false, stick, position);
                } else if dash_pressed {
                    self.start_dash(MoveState::Dash, stick);
                    self.ground_dash(dt);
                } else {
                    self.velocity.y = -STICK_DOWN_SPEED * self.tuning.scale;
                }
            }
            MoveState::Dash => {
                self.dash_time += dt;
                // A jump pressed during the dash's first 0.1 s waits for it (RiftApart.exe). It
                // takes off with the dash's speed, which the jump then blends away.
                self.dash_jump_queued |= pressed;
                if self.dash_jump_queued && self.dash_time >= DASH_JUMP_AFTER {
                    self.dash_jump_queued = false;
                    self.start_jump(dt, MoveState::Jump, position);
                } else if self.melee_wanted() && self.start_swing(self.combo_swing(), stick) {
                    self.swing(dt, false, stick, position);
                } else {
                    self.ground_dash(dt);
                }
            }
            MoveState::Melee => {
                // A dash press during a swing is lost: the game's melee state never checks it.
                self.swing(dt, pressed, stick, position);
            }
            MoveState::Smash => {
                self.air_time += dt;
                self.smash(dt);
            }
            MoveState::SmashLand => {
                self.smash_landing(dt, pressed, stick, position);
            }
            MoveState::AirDash => {
                self.air_time += dt;
                if self.try_grab_ledge(world, position, contact.ground_distance, dt) {
                } else if melee_pressed && self.can_smash(contact) {
                    self.start_smash(dt);
                } else {
                    self.air_dash(dt);
                }
            }
            MoveState::Jump | MoveState::DoubleJump | MoveState::WallJump | MoveState::LedgeJump | MoveState::SwingJump => {
                let params = self.params(self.state).clone();
                self.air_time += dt;
                self.extend_jump(input.jump, &params, position);
                if self.state == MoveState::SwingJump {
                    self.swing_jump_air(dt, stick);
                } else {
                    self.jump_air(dt, stick, &params);
                }
                self.integrate_vertical(dt, self.jump_gravity, params.terminal_velocity);
                // Ledges come first (RiftApart.exe), on the way down.
                if (self.velocity.y < 0.0 && self.try_grab_ledge(world, position, contact.ground_distance, dt)) || self.try_wall_stick(contact, position) {
                } else if pressed && self.can_midair_jump(&params, position, contact) {
                    self.start_jump(dt, MoveState::DoubleJump, position);
                } else if melee_pressed && self.can_smash(contact) {
                    self.start_smash(dt);
                } else if dash_pressed && !self.air_dash_used {
                    self.start_dash(MoveState::AirDash, stick);
                    self.air_dash(dt);
                } else if self.can_glide(input.jump, &params, contact) {
                    self.enter_glide();
                }
                // Outside a wall-jump zone, a wall jump coming down toward a floor rather than a
                // wall becomes a fall he can steer (RiftApart.exe).
                if self.state == MoveState::WallJump && self.velocity.y < 0.0 && !contact.wall_jump_zone && contact.landing_ahead {
                    self.enter_fall();
                }
                // The swing jump hands over to a fall at its top (RiftApart.exe).
                if self.state == MoveState::SwingJump && self.velocity.y < 0.0 {
                    self.enter_fall();
                }
                // Any jump still coming down after its NoFallTime does too, unless he's within
                // 0.5 m of the ground (RiftApart.exe).
                if matches!(self.state, MoveState::Jump | MoveState::DoubleJump | MoveState::WallJump | MoveState::LedgeJump)
                    && self.velocity.y < 0.0
                    && self.air_time >= params.no_fall_time
                    && contact.ground_distance.is_none_or(|d| d.abs() > NO_FALL_GROUND * self.tuning.scale)
                {
                    self.enter_fall();
                }
            }
            MoveState::Fall => {
                let params = self.tuning.fall.clone();
                self.air_time += dt;
                self.air_steer(dt, stick);
                self.fall_under(dt, &params);
                if (self.velocity.y < 0.0 && self.try_grab_ledge(world, position, contact.ground_distance, dt)) || self.try_wall_stick(contact, position) {
                } else if pressed && self.can_midair_jump(&params, position, contact) {
                    self.start_jump(dt, MoveState::DoubleJump, position);
                } else if melee_pressed && self.can_smash(contact) {
                    self.start_smash(dt);
                } else if dash_pressed && !self.air_dash_used {
                    self.start_dash(MoveState::AirDash, stick);
                    self.air_dash(dt);
                } else if self.can_glide(input.jump, &params, contact) {
                    self.enter_glide();
                }
            }
            MoveState::WallStick => {
                self.wall_time += dt;
                if self.jump_press_age <= WALL_JUMP_BUFFER && (self.wall_marked || self.wall_time > PLAIN_WALL_JUMP_DELAY) {
                    self.jump_off_wall(dt, position);
                } else if (self.wall_time > dt && contact.wall.is_none())
                    || contact.ground_distance.is_some_and(|d| d < self.tuning.wall_stick.min_ground_height)
                {
                    // Slid off the wall's edge, or down near the ground (RiftApart.exe: below
                    // WallStickMinGroundHeight he lets go).
                    self.enter_fall();
                } else {
                    self.wall_slide(dt);
                }
            }
            MoveState::Glide => {
                let params = self.tuning.glide.clone();
                self.air_time += dt;
                let speeds = self.tuning.glide_speeds.clone();
                let accel = self.tuning.glide_accel;
                self.steer(dt, stick, speeds.speed_for_stick(stick.length()), accel, accel);
                self.integrate_vertical(dt, params.gravity(), self.glide_terminal);
                if self.try_grab_ledge(world, position, contact.ground_distance, dt) || self.try_wall_stick(contact, position) {
                    self.events.push(Event::GlideEnded);
                } else if melee_pressed && self.can_smash(contact) {
                    self.events.push(Event::GlideEnded);
                    self.start_smash(dt);
                } else if dash_pressed && !self.air_dash_used {
                    self.events.push(Event::GlideEnded);
                    self.start_dash(MoveState::AirDash, stick);
                    self.air_dash(dt);
                } else if !input.jump {
                    // Letting go drops into a normal fall; a fresh press can still double jump
                    // if it wasn't used yet.
                    self.state = MoveState::Fall;
                    self.events.push(Event::GlideEnded);
                }
            }
            MoveState::LedgeHang => {
                let (jump_age, crouch_age) = (self.jump_press_age, self.crouch_press_age);
                self.ledge_hang(dt, jump_age, crouch_age, stick, world, position, contact.grounded);
            }
            MoveState::LedgeClimb => self.ledge_climb(dt, position, contact.grounded),
            MoveState::Swing => self.swing_tick(dt, input, pressed, position, world),
            MoveState::Hook => self.hook_tick(dt, input, position, world),
            MoveState::LedgeDrop => self.ledge_drop(dt, position, contact.grounded),
        }

        self.turn(dt);
        // The tick's average vertical speed, unless a move started after it was worked out
        // (an air dash, a wall stick or a ledge grab set their own).
        let vy = match self.move_vy.take() {
            Some((after, average)) if after == self.velocity.y => average,
            _ => self.velocity.y,
        };
        Vec3::new(self.velocity.x, vy, self.velocity.z)
    }

    /// Starts combo swing `index`, aimed where the stick points (straight ahead without input).
    /// Returns false when there's no such swing.
    fn start_swing(&mut self, index: usize, stick: Vec2) -> bool {
        if index >= self.tuning.melee.combo.len() {
            return false;
        }
        if stick.length() > 0.1 {
            self.facing_yaw = stick.x.atan2(stick.y);
        }
        self.state = MoveState::Melee;
        self.swing = index;
        self.swing_time = 0.0;
        self.swing_queued = false;
        self.since_melee_press = f32::INFINITY;
        self.combo_alive = 0.0;
        self.swing_hit = false;
        self.extending = false;
        self.events.push(Event::MeleeSwing { index });
        true
    }

    /// A melee press now, or one made within `press_buffer` that nothing used yet; none while
    /// the combo rests after its last swing.
    fn melee_wanted(&self) -> bool {
        self.combo_rest <= 0.0 && self.since_melee_press <= self.tuning.melee.press_buffer
    }

    /// The swing a press starts: the next one while the combo is alive, else the first.
    fn combo_swing(&self) -> usize {
        if self.combo_alive > 0.0 { self.combo_next } else { 0 }
    }

    /// The current swing is over (played out or cut short): the combo stays alive for
    /// `combo_timeout`, or after its last swing it starts over, resting `combo_cooldown` first
    /// if that swing didn't reach its first AllowEarlyTransition (RiftApart.exe's melee
    /// component).
    fn end_swing(&mut self) {
        let m = &self.tuning.melee;
        if self.swing + 1 >= m.combo.len() {
            if self.swing_time < m.combo[self.swing].early_exit {
                self.combo_rest = m.combo_cooldown;
            }
            self.combo_next = 0;
            self.combo_alive = 0.0;
        } else {
            self.combo_next = self.swing + 1;
            self.combo_alive = m.combo_timeout;
        }
        self.swing_queued = false;
        self.state = MoveState::Ground;
    }

    /// One tick of a swing, timed by its clip's events like RiftApart.exe's melee state: the
    /// lunge along the facing direction (the clip's root motion) and the hit.
    /// - A press (or one made up to `press_buffer` earlier) asks for the next swing, which
    ///   starts at the first AllowEarlyTransition (`early_exit`), or right away once that has
    ///   passed. The last swing takes no press: from its `early_exit` (and again from its
    ///   `exit`) the combo rests for `combo_cooldown`.
    /// - A jump cuts the swing short at any time; from `early_exit` the stick does too, unless
    ///   the next swing was asked for.
    /// - At the second AllowEarlyTransition (`exit`) the swing is over.
    fn swing(&mut self, dt: f32, jump_pressed: bool, stick: Vec2, position: Vec3) {
        let m = &self.tuning.melee;
        let (cooldown, last) = (m.combo_cooldown, self.swing + 1 >= m.combo.len());
        let w: SwingParams = m.combo[self.swing].clone();
        let before = self.swing_time;
        self.swing_time += dt;
        let reached = |t: f32| before < t && t <= before + dt;
        if last && (reached(w.early_exit) || reached(w.exit)) {
            self.combo_rest = cooldown;
        }
        if !last && self.melee_wanted() {
            self.swing_queued = true;
            self.since_melee_press = f32::INFINITY;
        }
        if !self.swing_hit && self.swing_time >= w.hit_time {
            self.swing_hit = true;
            self.events.push(Event::MeleeHit { index: self.swing });
        }
        let forward = (w.lunge_at(self.swing_time) - w.lunge_at(before)) / dt;
        let v = self.facing_dir() * forward;
        self.velocity = Vec3::new(v.x, -STICK_DOWN_SPEED * self.tuning.scale, v.z);

        let free = self.swing_time >= w.early_exit;
        if jump_pressed {
            self.end_swing();
            self.start_jump(dt, MoveState::Jump, position);
        } else if free && self.swing_queued {
            let next = self.swing + 1;
            self.end_swing();
            self.start_swing(next, stick);
        } else if (free && stick.length() > MOVE_STICK) || self.swing_time >= w.exit {
            self.end_swing();
        }
    }

    fn can_smash(&self, contact: &Contact) -> bool {
        contact.ground_distance.is_none_or(|d| d >= self.tuning.melee.smash_min_height)
    }

    /// Hyper-Strike: a short hop (its own jump config), then a slam straight down.
    fn start_smash(&mut self, dt: f32) {
        let hop = self.tuning.melee.smash_hop.clone();
        let g = hop.gravity_for(hop.height_min);
        self.state = MoveState::Smash;
        self.swing_time = 0.0;
        self.since_melee_press = f32::INFINITY;
        self.smash_descending = false;
        self.extending = false;
        self.jump_gravity = g;
        self.velocity.y = (2.0 * g * hop.height_min).sqrt();
        self.events.push(Event::SmashStarted);
        self.smash(dt);
    }

    /// Rising: drifts forward by the intro animation's root motion. From the top of the hop:
    /// straight down at the slam gravity. (Assumption: the slam has no sideways movement.)
    fn smash(&mut self, dt: f32) {
        let m = self.tuning.melee.clone();
        let before = self.swing_time;
        self.swing_time += dt;
        if !self.smash_descending {
            let forward = (m.smash_intro.lunge_at(self.swing_time) - m.smash_intro.lunge_at(before)) / dt;
            let v = self.facing_dir() * forward;
            self.velocity.x = v.x;
            self.velocity.z = v.z;
            self.integrate_vertical(dt, self.jump_gravity, m.smash_hop.terminal_velocity);
            if self.velocity.y <= 0.0 {
                self.smash_descending = true;
            }
        } else {
            self.velocity.x = 0.0;
            self.velocity.z = 0.0;
            self.integrate_vertical(dt, m.smash_gravity, m.smash_hop.terminal_velocity);
        }
    }

    fn smash_land(&mut self) {
        let fall_speed = -self.velocity.y;
        self.land();
        self.state = MoveState::SmashLand;
        self.swing_time = 0.0;
        self.velocity = Vec3::ZERO;
        self.events.push(Event::SmashImpact { fall_speed });
    }

    /// Stays put until `smash_land_time`; from `smash_land_early_exit` the stick, a jump or a
    /// swing cut it short.
    fn smash_landing(&mut self, dt: f32, jump_pressed: bool, stick: Vec2, position: Vec3) {
        self.swing_time += dt;
        self.velocity = Vec3::new(0.0, -STICK_DOWN_SPEED * self.tuning.scale, 0.0);
        let m = &self.tuning.melee;
        let can_cut = self.swing_time >= m.smash_land_early_exit;
        // A jump first, so a press on the tick it ends isn't lost.
        if can_cut && jump_pressed {
            self.state = MoveState::Ground;
            self.start_jump(dt, MoveState::Jump, position);
        } else if self.swing_time >= m.smash_land_time || (can_cut && stick.length() > MOVE_STICK) {
            self.state = MoveState::Ground;
        } else if can_cut && self.melee_wanted() && self.start_swing(0, stick) {
            self.swing(dt, false, stick, position);
        }
    }

    fn params(&self, state: MoveState) -> &JumpParams {
        match state {
            MoveState::DoubleJump if self.jump_aimed => &self.tuning.aim_double_jump,
            MoveState::DoubleJump => &self.tuning.double_jump,
            MoveState::Fall => &self.tuning.fall,
            MoveState::Glide => &self.tuning.glide,
            MoveState::WallJump => &self.tuning.wall_jump,
            MoveState::LedgeJump => &self.tuning.ledge_jump,
            MoveState::SwingJump => &self.tuning.swingshot.jump,
            _ if self.jump_aimed => &self.tuning.aim_single_jump,
            _ => &self.tuning.single_jump,
        }
    }

    /// The speeds a jump steers by: its kind's own set where the game gives it one (the wall
    /// jump's WallJumpSpeeds), else the jump speeds (RiftApart.exe).
    fn jump_speeds(&self) -> &SpeedParams {
        match self.state {
            MoveState::WallJump => &self.tuning.wall_jump_speeds,
            MoveState::LedgeJump => &self.tuning.ledge_jump_speeds,
            _ => &self.tuning.air_speeds,
        }
    }

    /// Ground speed for the stick: the normal speeds (also when strafing while firing), or in
    /// aim mode the aim speeds, slower when backing away from the aim direction.
    fn ground_speed_for(&self, stick: Vec2) -> f32 {
        let Some(aim) = self.aiming.filter(|_| self.aim_mode) else {
            return self.tuning.ground_speeds.speed_for_stick(stick.length());
        };
        let forward = self.tuning.aim_speeds.speed_for_stick(stick.length());
        let back = self.tuning.aim_back_speed.min(forward);
        let along = stick.normalize_or_zero().dot(aim);
        if along >= 0.0 { forward } else { forward + (back - forward) * -along }
    }

    /// Starts a jump aimed at its tap height; holding raises it (`extend_jump`). Gravity comes
    /// from the tap height, so every jump of a kind peaks at `time_to_peak` when tapped
    /// (RiftApart.exe).
    fn start_jump(&mut self, dt: f32, kind: MoveState, position: Vec3) {
        self.jump_press_age = f32::INFINITY;
        self.jump_aimed = self.aim_mode;
        let params = self.params(kind).clone();
        let (mut h_min, mut h_max) = (params.height_min, params.height_max);

        if kind == MoveState::DoubleJump {
            self.midair_jumps += 1;
            // Both heights are clamped to what's left of the chain's total; already past it,
            // the jump gets a fixed 0.5 m. Gravity follows the clamped height, so the timing
            // stays the same (RiftApart.exe). The game only switches to 0.5 m below zero room;
            // exactly zero would mean zero gravity here, so it's included.
            if let (Some(cap), Some(origin)) = (params.max_total_jump, self.chain_origin_y) {
                let room = origin + cap - position.y;
                if room <= 0.0 {
                    h_min = OVER_CAP_DOUBLE_JUMP * self.tuning.scale;
                    h_max = h_min;
                } else {
                    h_min = h_min.min(room);
                    h_max = h_max.min(room);
                }
            }
            self.events.push(Event::DoubleJumped);
        } else {
            self.chain_origin_y = Some(position.y);
            self.events.push(Event::Jumped);
        }

        // Take-off (RiftApart.exe): from the speed he has, toward where the stick points, or
        // ahead without input. `jump_air` does the rest.
        let flat = Vec2::new(self.velocity.x, self.velocity.z);
        self.jump_takeoff_speed = flat.length();
        self.jump_stick = self.stick.length().min(1.0);
        self.jump_dir = if self.jump_stick >= MOVE_STICK { self.stick.normalize() } else { Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos()) };
        self.jump_first = true;
        self.jump_carry = true;
        self.jump_keep_base = false;
        let g = params.gravity_for(h_min);
        self.state = kind;
        self.air_time = 0.0;
        self.extending = true;
        self.jump_origin_y = position.y;
        self.jump_height_min = h_min;
        self.jump_height_max = h_max;
        self.jump_gravity = g;
        // Not confirmed: the game adds the take-off speed to the state's vertical speed, which
        // probably starts at zero. We replace it.
        self.velocity.y = (2.0 * g * h_min).sqrt();
        // The jump starts at the beginning of this tick, so gravity already applies to it.
        self.integrate_vertical(dt, g, params.terminal_velocity);
    }

    fn start_dash(&mut self, kind: MoveState, stick: Vec2) {
        // Dash where the stick points, or straight ahead without input (RiftApart.exe: below
        // the moving threshold; ahead is the aim while strafing).
        let ahead = self.aiming.unwrap_or(Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos()));
        self.dash_dir = if stick.length() > MOVE_STICK { stick.normalize() } else { ahead };
        let current = Vec2::new(self.velocity.x, self.velocity.z).dot(self.dash_dir).max(0.0);
        let params = self.dash_params(kind);
        self.dash_speed = if params.accel > 0.0 { current } else { params.max_speed };
        self.dash_travelled = 0.0;
        self.dash_braking = false;
        self.dash_time = 0.0;
        self.dash_jump_queued = false;
        self.extending = false;
        self.state = kind;
        if kind == MoveState::AirDash {
            self.air_dash_used = true;
            self.events.push(Event::AirDashed);
        } else {
            self.events.push(Event::Dashed);
        }
        // Strafing when it starts, he keeps facing the aim through the dash, which then goes
        // sideways or back relative to him; otherwise he faces where it goes, right away
        // (RiftApart.exe; see `turn`).
        self.dash_strafe = self.aiming.is_some();
        if !self.dash_strafe {
            self.facing_yaw = self.dash_dir.x.atan2(self.dash_dir.y);
        }
    }

    fn dash_params(&self, kind: MoveState) -> DashParams {
        if kind == MoveState::AirDash { self.tuning.air_dash.clone() } else { self.tuning.ground_dash.clone() }
    }

    /// Speeds up towards `max_speed`, and starts braking just in time to be back at run speed
    /// when `distance` is covered. (Assumption: `distance` includes the braking.)
    fn ground_dash(&mut self, dt: f32) {
        let p = self.tuning.ground_dash.clone();
        let end_speed = self.tuning.ground_speeds.run_max;
        let remaining = p.distance - self.dash_travelled;
        let brake_distance = if p.decel > 0.0 { (self.dash_speed.powi(2) - end_speed.powi(2)).max(0.0) / (2.0 * p.decel) } else { 0.0 };
        if !self.dash_braking && remaining <= brake_distance {
            self.dash_braking = true;
        }
        if self.dash_braking {
            self.dash_speed = if p.decel > 0.0 { (self.dash_speed - p.decel * dt).max(end_speed) } else { end_speed };
        } else {
            self.dash_speed = if p.accel > 0.0 { (self.dash_speed + p.accel * dt).min(p.max_speed) } else { p.max_speed };
        }
        self.dash_travelled += self.dash_speed * dt;
        if self.dash_travelled >= p.distance || (self.dash_braking && self.dash_speed <= end_speed) {
            self.dash_speed = self.dash_speed.min(end_speed);
            self.state = MoveState::Ground;
        }
        let v = self.dash_dir * self.dash_speed;
        self.velocity = Vec3::new(v.x, -STICK_DOWN_SPEED * self.tuning.scale, v.y);
    }

    /// Flat dash at `max_speed` for `distance`, then a normal fall at no more than the air
    /// speed. (Assumption: no gravity during the air dash.)
    fn air_dash(&mut self, dt: f32) {
        let p = self.tuning.air_dash.clone();
        self.dash_speed = if p.accel > 0.0 { (self.dash_speed + p.accel * dt).min(p.max_speed) } else { p.max_speed };
        self.dash_travelled += self.dash_speed * dt;
        let v = self.dash_dir * self.dash_speed;
        self.velocity = Vec3::new(v.x, 0.0, v.y);
        if self.dash_travelled >= p.distance {
            let v = v.clamp_length_max(self.tuning.air_speeds.run_max);
            self.velocity = Vec3::new(v.x, 0.0, v.y);
            self.state = MoveState::Fall;
            self.air_time = 0.0;
        }
    }

    /// While the button stays down (until `button_max_hold`), the apex target grows from the
    /// tap height to the held height and the rise is re-aimed at it every tick:
    /// `v = sqrt(2·g·(target − risen))`. Letting go freezes the rise (RiftApart.exe).
    fn extend_jump(&mut self, held: bool, params: &JumpParams, position: Vec3) {
        if !self.extending {
            return;
        }
        if !held || self.velocity.y <= 0.0 {
            self.extending = false;
            return;
        }
        let t = params.hold_fraction(self.air_time);
        let target = self.jump_height_min + (self.jump_height_max - self.jump_height_min) * t;
        let risen = position.y - self.jump_origin_y;
        self.velocity.y = (2.0 * self.jump_gravity * (target - risen)).max(0.0).sqrt();
        if self.air_time > params.button_max_hold {
            self.extending = false;
        }
    }

    fn fall_under(&mut self, dt: f32, params: &JumpParams) {
        self.integrate_vertical(dt, params.gravity(), params.terminal_velocity);
    }

    /// Gravity with a fall-speed limit. Moves by the average speed over the tick, which is
    /// exact under constant gravity and is what the game does too (RiftApart.exe).
    fn integrate_vertical(&mut self, dt: f32, gravity: f32, terminal: f32) {
        let before = self.velocity.y;
        self.velocity.y = (before - gravity * dt).max(-terminal);
        self.move_vy = Some((self.velocity.y, (before + self.velocity.y) * 0.5));
    }

    fn can_midair_jump(&self, params: &JumpParams, position: Vec3, contact: &Contact) -> bool {
        if self.midair_jumps >= self.tuning.max_midair_jumps || self.air_time < params.double_jump_time_start {
            return false;
        }
        let height = match self.state {
            MoveState::Fall => contact.ground_distance.unwrap_or(f32::MAX),
            _ => position.y - self.jump_origin_y,
        };
        // On the way down the height check no longer matters; it only stops instant double taps.
        height >= params.double_jump_height_min || self.velocity.y <= 0.0
    }

    fn can_glide(&self, held: bool, params: &JumpParams, contact: &Contact) -> bool {
        if !held || self.velocity.y > 0.0 || self.air_time < params.no_glide_time {
            return false;
        }
        // The glide config's height minimum keeps you from gliding a step above the floor.
        let min_height = self.tuning.glide.double_jump_height_min;
        contact.ground_distance.is_none_or(|d| d >= min_height)
    }

    /// The glide's fall-speed limit is the current fall speed clamped to 4..12 m/s (scaled) and
    /// stays for the whole glide, so starting it near a jump's peak sinks slowly and starting it
    /// late keeps more speed (RiftApart.exe).
    fn enter_glide(&mut self) {
        let s = self.tuning.scale;
        let fall = (-self.velocity.y).max(0.0);
        self.glide_terminal = fall.clamp(GLIDE_ENTRY_FALL_MIN * s, GLIDE_ENTRY_FALL_MAX * s);
        self.velocity.y = self.velocity.y.max(-self.glide_terminal).min(0.0);
        self.state = MoveState::Glide;
        self.extending = false;
        self.events.push(Event::GlideStarted);
    }

    /// Airborne against a wall, at least WallStickMinGroundHeight above the ground, he sticks
    /// to it (RiftApart.exe):
    /// - a wall that takes wall jumps, whichever way he faces, any number of times;
    /// - a plain one only facing, or pushing the stick, within WallStickFacingIntoWallThreshold
    ///   of straight into it, and only if he hasn't stuck to a wall yet this air time.
    ///
    /// Not to any wall near the last jump-off for WALL_BLOCK_TIME.
    fn try_wall_stick(&mut self, contact: &Contact, position: Vec3) -> bool {
        let Some(wall) = contact.wall.filter(|w| w.normal.length_squared() > 0.5) else {
            return false;
        };
        let p = &self.tuning.wall_stick;
        if contact.ground_distance.is_some_and(|d| d < p.min_ground_height) {
            return false;
        }
        if let Some((from, left)) = self.wall_block
            && left > 0.0
        {
            // The block is a box around the jump-off point, aligned to that wall.
            let d = Vec2::new(position.x - from.x, position.z - from.z);
            let across = self.wall_normal;
            let along = Vec2::new(-across.y, across.x);
            let reach = WALL_BLOCK_REACH * self.tuning.scale;
            if d.dot(across).abs() <= reach && d.dot(along).abs() <= reach {
                return false;
            }
        }
        if !wall.sticky {
            if self.wall_sticks > 0 {
                return false;
            }
            let into = -wall.normal;
            let cos = p.facing_threshold.to_radians().cos();
            let facing = Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos());
            let pushing = self.stick.length() > MOVE_STICK && self.stick.normalize().dot(into) > cos;
            if facing.dot(into) <= cos && !pushing {
                return false;
            }
        }
        self.wall_sticks += 1;
        self.wall_marked = wall.sticky;
        self.wall_up_ok = self.midair_jumps < self.tuning.max_midair_jumps;
        self.wall_normal = wall.normal;
        // The game's right clips when the wall faces against the camera's right.
        self.wall_right = wall.normal.dot(self.view_right) <= 0.0;
        self.wall_time = 0.0;
        self.state = MoveState::WallStick;
        self.velocity = Vec3::ZERO;
        self.extending = false;
        self.events.push(Event::WallStuck);
        true
    }

    /// Jumping on a wall (RiftApart.exe): off it, or, on a plain wall with the stick pushed into
    /// it and his double jump unused, up it, as a double jump with at least
    /// WallJumpUpMinHeightGain of the chain's MaxTotalJump left. Either way, nearby walls are
    /// blocked for WALL_BLOCK_TIME.
    fn jump_off_wall(&mut self, dt: f32, position: Vec3) {
        self.jump_press_age = f32::INFINITY;
        self.wall_block = Some((position, WALL_BLOCK_TIME));
        let into = self.stick.length() >= MOVE_STICK && self.stick.normalize().dot(self.wall_normal) <= WALL_UP_STICK;
        if self.wall_marked || !into || !self.wall_up_ok {
            self.start_wall_jump(dt, position);
            return;
        }
        if let Some(cap) = self.tuning.double_jump.max_total_jump {
            let gain = self.tuning.wall_stick.up_min_height_gain;
            let used = self.chain_origin_y.map_or(0.0, |o| (position.y - o).max(0.0)).min((cap - gain).max(0.0));
            self.chain_origin_y = Some(position.y - used);
        }
        self.velocity = Vec3::ZERO;
        self.start_jump(dt, MoveState::DoubleJump, position);
    }

    /// Holding on for WallStickNoSlideTime, then sliding down at WallStickGravity up to
    /// WallStickTerminalVelocity, pressed against the wall.
    fn wall_slide(&mut self, dt: f32) {
        let p = &self.tuning.wall_stick;
        let vy = if self.wall_time < p.no_slide_time { 0.0 } else { (self.velocity.y - p.slide_gravity * dt).max(-p.slide_terminal) };
        let push = -self.wall_normal * WALL_PUSH * self.tuning.scale;
        self.velocity = Vec3::new(push.x, vy, push.y);
    }

    /// The jump off a wall: the WallJumpConfig jump, straight away from the wall at the
    /// WallJumpSpeeds run speed, which it keeps for NoInputTime. It starts a new jump, so his double jump and air
    /// dash are back (ours: the game starts a fresh jump state; how it counts air moves across
    /// it isn't read).
    fn start_wall_jump(&mut self, dt: f32, position: Vec3) {
        self.velocity = Vec3::ZERO;
        self.midair_jumps = 0;
        self.air_dash_used = false;
        self.start_jump(dt, MoveState::WallJump, position);
        self.jump_dir = self.wall_normal;
        self.jump_stick = 1.0;
        self.jump_keep_base = true;
        // Its clip ends in the stick pose of the wall ahead (taken as facing back at this one),
        // so it takes that wall's side. He turns off the wall toward the camera: along it, to
        // his left (facing it) for the right clip, his right for the left one (RiftApart.exe);
        // the air turn then brings him round to where he's going. Without a camera he just
        // faces the way he jumps.
        let out = self.wall_normal;
        self.wall_right = out.dot(self.view_right) >= 0.0;
        let left = Vec2::new(-out.y, out.x);
        let face = if self.view_right.length_squared() < 0.25 {
            out
        } else if self.wall_right {
            left
        } else {
            -left
        };
        self.facing_yaw = face.x.atan2(face.y);
        self.events.push(Event::WallJumped);
    }

    /// How fast he's pulled down right now (m/s²), e.g. for the host's `Contact::landing_ahead`:
    /// his jump's pull in a jump or the Hyper-Strike's hop, the slam's on the way down, the
    /// fall's, the glide's or the swing's in those, and none on the ground or holding on.
    pub fn gravity(&self) -> f32 {
        match self.state {
            MoveState::Jump | MoveState::DoubleJump | MoveState::WallJump | MoveState::LedgeJump | MoveState::SwingJump => self.jump_gravity,
            MoveState::Smash if self.smash_descending => self.tuning.melee.smash_gravity,
            MoveState::Smash => self.jump_gravity,
            MoveState::Fall => self.tuning.fall.gravity(),
            MoveState::Glide => self.tuning.glide.gravity(),
            MoveState::Swing => self.tuning.swingshot.gravity,
            _ => 0.0,
        }
    }

    /// The wall he's stuck to, or last jumped off: the way it faces (out of it, x/z).
    pub fn wall_normal(&self) -> Vec2 {
        self.wall_normal
    }

    /// Which of the game's wall clips fit: the right ones (true) or the left. On a wall, the
    /// right ones when it faces against the camera's right (`Input::view_right`); in a wall
    /// jump, those of the wall ahead.
    pub fn wall_right(&self) -> bool {
        self.wall_right
    }

    /// A fall with the velocity he has, upward speed too: the game's fall takes the measured
    /// velocity (RiftApart.exe).
    fn enter_fall(&mut self) {
        self.state = MoveState::Fall;
        self.air_time = 0.0;
        self.extending = false;
        self.chain_origin_y = None;
    }

    fn land(&mut self) {
        if self.state == MoveState::Glide {
            self.events.push(Event::GlideEnded);
        }
        self.events.push(Event::Landed { fall_speed: -self.velocity.y });
        self.state = MoveState::Ground;
        self.midair_jumps = 0;
        self.air_dash_used = false;
        self.chain_origin_y = None;
        self.extending = false;
        self.air_time = 0.0;
        self.velocity.y = 0.0;
        self.wall_block = None;
        self.wall_sticks = 0;
        self.left_ledge = None;
        self.walk_off_time = 0.0;
        self.hold = None;
        self.ledge_move = None;
        self.swingshot = None;
    }

    fn air_steer(&mut self, dt: f32, stick: Vec2) {
        let current = Vec2::new(self.velocity.x, self.velocity.z);
        let accel = self.tuning.air_accel;
        if stick.length() < 1e-3 {
            // No input: keep drifting, bleeding a little speed.
            let len = (current.length() - accel * 0.25 * dt).max(0.0);
            let next = current.normalize_or_zero() * len;
            self.velocity.x = next.x;
            self.velocity.z = next.y;
            return;
        }
        let speeds = &self.tuning.air_speeds;
        // Don't brake below the take-off speed (up to the air maximum); only steer.
        let speed = speeds.speed_for_stick(stick.length()).max(current.length().min(speeds.run_max));
        self.steer(dt, stick, speed, accel, accel * 0.25);
    }

    /// A jump's horizontal speed, the game's way (RiftApart.exe).
    /// - The first tick goes on at the speed he took off with (unclamped: a dash's too),
    ///   toward where the stick pointed.
    /// - After that, each tick blends from the speed he has toward the stick's: the jump speeds
    ///   for its push, times the push again. The share of the old speed kept is the jump's
    ///   momentum (`JumpParams::momentum_keep`), almost none for a single jump.
    /// - With the stick left alone since take-off, it keeps up to the jump run speed ahead;
    ///   let go after steering, it stops.
    fn jump_air(&mut self, dt: f32, stick: Vec2, params: &JumpParams) {
        let v_last = Vec2::new(self.velocity.x, self.velocity.z);
        // The ledge jump never takes the stick: its speed holds for the whole jump (RiftApart.exe).
        let steerable = self.state != MoveState::LedgeJump && self.air_time >= params.no_input_time;
        let (push, dir) = if self.jump_first || !steerable {
            (self.jump_stick, self.jump_dir)
        } else {
            (stick.length().min(1.0), stick.normalize_or_zero())
        };
        let dir = if push < 0.01 { if self.jump_carry { self.jump_dir } else { Vec2::new(self.facing_yaw.sin(), self.facing_yaw.cos()) } } else { dir };
        let speeds = self.jump_speeds().clone();
        let (m, speed) = if push <= MOVE_STICK {
            (1.0, if self.jump_carry { v_last.length().min(speeds.run_max) } else { 0.0 })
        } else {
            if steerable && !self.jump_first {
                self.jump_carry = false;
            }
            (push, speeds.speed_for_stick(push))
        };
        let base = dir.normalize_or_zero() * speed;
        let v = if self.jump_first {
            // Wall jumps (like the game's ledge, vault and grind jumps) go at the jump speeds.
            self.jump_held = if self.jump_keep_base { base } else { base.normalize_or_zero() * self.jump_takeoff_speed.max(v_last.length()) };
            self.jump_held
        } else if !steerable {
            self.jump_held
        } else {
            let target = base * m;
            let keep = if self.state == MoveState::WallJump && !self.in_wall_jump_zone {
                let (lo, hi) = WALL_JUMP_MOMENTUM;
                JumpParams { momentum_weight_min: lo, momentum_weight_max: hi, ..params.clone() }.momentum_keep(self.air_time, dt)
            } else {
                params.momentum_keep(self.air_time, dt)
            };
            target + (v_last - target) * keep
        };
        self.jump_first = false;
        self.velocity.x = v.x;
        self.velocity.z = v.y;
    }

    /// Moves horizontal velocity towards `stick` direction × `speed`, accelerating at `accel`
    /// and slowing down at `decel`.
    fn steer(&mut self, dt: f32, stick: Vec2, speed: f32, accel: f32, decel: f32) {
        let current = Vec2::new(self.velocity.x, self.velocity.z);
        let target = stick.normalize_or_zero() * speed;
        let rate = if target.length_squared() >= current.length_squared() { accel } else { decel };
        let delta = target - current;
        let step = rate * dt;
        let next = if delta.length() <= step { target } else { current + delta / delta.length() * step };
        self.velocity.x = next.x;
        self.velocity.z = next.y;
    }

    /// Turns towards the movement direction, or while aiming towards the aim (twice as fast).
    /// Turns Ratchet toward where he should face: the aim while strafing, else where he's
    /// going. On the ground this is the game's mover turn (RiftApart.exe): each tick takes an
    /// exponential share of the angle left (gain) plus part of the last tick's turn (damp),
    /// capped per second and never past the target; moving, a 180° turn is 90 % done in about
    /// a quarter second, standing it's nearly a snap. In the air and during moves, a plain turn
    /// rate (ours).
    fn turn(&mut self, dt: f32) {
        // Dashing, he faces the aim (snapped, every tick) if he was strafing when the dash
        // started, else where the dash goes (RiftApart.exe).
        // On a wall he faces it.
        if matches!(self.state, MoveState::LedgeHang | MoveState::LedgeClimb | MoveState::LedgeDrop | MoveState::Swing | MoveState::Hook) {
            self.turn_omega = 0.0;
            return;
        }
        if self.state == MoveState::WallStick {
            self.facing_yaw = (-self.wall_normal.x).atan2(-self.wall_normal.y);
            self.turn_omega = 0.0;
            return;
        }
        if matches!(self.state, MoveState::Dash | MoveState::AirDash) {
            let toward = if self.dash_strafe { self.aiming } else { Some(self.dash_dir) };
            if let Some(d) = toward {
                self.facing_yaw = d.x.atan2(d.y);
            }
            self.turn_omega = 0.0;
            return;
        }
        let flat = Vec2::new(self.velocity.x, self.velocity.z);
        let moving = self.stick.length() > MOVE_STICK;
        let target = match self.aiming {
            Some(aim) => aim.x.atan2(aim.y),
            None if self.state == MoveState::Ground && moving => self.stick.x.atan2(self.stick.y),
            None if flat.length() >= 0.3 * self.tuning.scale => flat.x.atan2(flat.y),
            None => {
                self.turn_omega = 0.0;
                return;
            }
        };
        let diff = wrap_angle(target - self.facing_yaw);
        if self.state != MoveState::Ground {
            let rate = if self.aiming.is_some() { self.tuning.turn_rate * 2.0 } else { self.tuning.turn_rate };
            self.facing_yaw += diff.clamp(-rate * dt, rate * dt);
            self.turn_omega = 0.0;
            return;
        }
        if diff.abs() <= TURN_SNAP {
            self.facing_yaw = target;
            self.turn_omega = 0.0;
            return;
        }
        let (gain, damp, cap) = if moving { MOVING_TURN } else { STANDING_TURN };
        // Only the upper end is clamped (as in the game), so a turn of exactly 180° has a side.
        let err = diff.min(0.99 * std::f32::consts::PI);
        let carry = 0.5 * dt * self.turn_omega * (2.0 - (damp * dt / 30.0).exp());
        let step = (carry + (1.0 - (gain * dt).exp()) * err).clamp(-cap * dt, cap * dt).clamp(-err.abs(), err.abs());
        self.turn_omega = step / dt;
        self.facing_yaw = wrap_angle(self.facing_yaw + step);
    }
}

/// The same angle in -π..=π.
fn wrap_angle(mut a: f32) -> f32 {
    while a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    }
    while a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

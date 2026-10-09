//! The sandbox's clip choice (`ratchet_model.rs`), step by step in its order, against the Bevy
//! player in `mixer`: `setup_clips` builds the [`Graph`], `play_clips` is [`play_clips`], and
//! Bevy's systems after it run in [`Animator::update`].

use crate::clips::*;
use crate::mixer::{Player, Transitions};
use crate::{AnimInput, ClipInfo, Layer, LayerKind, Slot, SoundClock};
use libratchet::glam::{Vec2, Vec3};
use libratchet::{LedgeMove, MoveState, Ratchet};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::time::Duration;

/// A clip node of the sandbox's animation graph: the host's clip (none for the swing loop's
/// carrier, which only holds a weight), its length, and how it goes into the pose.
#[derive(Clone, Debug)]
struct Node {
    clip: Option<usize>,
    seconds: f32,
    kind: LayerKind,
}

/// The sandbox's animation graph (`setup_clips`): every clip node in the order it was made,
/// which is the order the pose blends them in, and where each layer is.
#[derive(Clone, Debug)]
struct Graph {
    nodes: Vec<Node>,
    /// The move clips (CLIP_STATES, and SWING_LOOP's carrier) by name.
    clips: HashMap<&'static str, usize>,
    /// Each run's two lean clips (left, right).
    lean_nodes: Vec<(&'static str, [usize; 2])>,
    /// The layers (see `clips`), with their lengths.
    draw_layers: [[Option<(usize, f32)>; 2]; 3],
    glide_lean: Option<(usize, f32)>,
    glove_hand: Option<usize>,
    catch_layer: Option<(usize, f32)>,
    grenade_layers: [Option<(usize, f32)>; 2],
    windup_layers: [Option<usize>; 2],
    aim_layers: [Option<(usize, f32)>; 8],
    throw_layer: Option<(usize, f32)>,
    fire_layer: Option<(usize, f32)>,
    /// The speed each clip was authored for (its root motion over its length, when it moves
    /// more than 0.5 m), to play it at the rate that keeps the feet planted.
    authored_speed: HashMap<String, f32>,
    /// The swing jump's JumpPeak event (seconds into its clip).
    swing_jump_peak: Option<f32>,
}

/// Makes the graph's nodes from the host's clips.
struct Builder<'a> {
    clips: &'a [ClipInfo],
    by_name: HashMap<&'a str, usize>,
    nodes: Vec<Node>,
}

impl Builder<'_> {
    fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// A node for the host's clip `name`, if it has one: the node and the clip's length.
    fn add(&mut self, name: &str, kind: LayerKind) -> Option<(usize, f32)> {
        let &clip = self.by_name.get(name)?;
        // (A length that makes no sense is none.)
        let seconds = Some(self.clips[clip].seconds).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(0.0);
        self.nodes.push(Node { clip: Some(clip), seconds, kind });
        Some((self.nodes.len() - 1, seconds))
    }
}

impl Graph {
    fn new(clips: &[ClipInfo]) -> Graph {
        let mut by_name = HashMap::new();
        for (i, c) in clips.iter().enumerate() {
            by_name.entry(c.name.as_str()).or_insert(i);
        }
        let mut b = Builder { clips, by_name, nodes: Vec::new() };
        // The move clips crossfade under the base blend; layers are added on top of it.
        let mut move_clips = HashMap::new();
        for &name in CLIP_STATES {
            if let Some((node, _)) = b.add(name, LayerKind::Base) {
                move_clips.insert(name, node);
            }
        }
        // The swing loop's own node: a clip with no tracks, faded in and out by the transitions
        // like any move clip, whose weight the loop's clips share (`blend_swing_loop`).
        if SWING_LOOP_CLIPS.iter().any(|(_, name)| move_clips.contains_key(name)) {
            b.nodes.push(Node { clip: None, seconds: 1.0, kind: LayerKind::Base });
            move_clips.insert(SWING_LOOP, b.nodes.len() - 1);
        }
        let mut lean_nodes = Vec::new();
        for (run, names) in LEAN_CLIPS {
            if names.iter().all(|name| b.has(name))
                && let (Some((left, _)), Some((right, _))) = (b.add(names[0], LayerKind::Base), b.add(names[1], LayerKind::Base))
            {
                lean_nodes.push((run, [left, right]));
            }
        }
        let mut draw_layers = [[None; 2]; 3];
        for (grip, names) in DRAW_LAYERS.iter().enumerate() {
            for (which, name) in names.iter().enumerate() {
                draw_layers[grip][which] = b.add(name, LayerKind::Additive);
            }
        }
        let glide_lean = b.add(GLIDE_LEAN, LayerKind::Additive);
        let glove_hand = b.add(GLOVE_HAND, LayerKind::Base).map(|(node, _)| node);
        // The catch leaves everything below the waist alone.
        let catch_layer = b.add(CATCH_CLIP, LayerKind::BaseUpperBody);
        let grenade_layers = GRENADE_LAYERS.map(|name| b.add(name, LayerKind::Additive));
        let windup_layers = WINDUP_LAYERS.map(|name| b.add(name, LayerKind::Additive).map(|(node, _)| node));
        let aim_layers = AIM_LAYERS.map(|name| b.add(name, LayerKind::Additive));
        let throw_layer = b.add(THROW_LAYER, LayerKind::Additive);
        let fire_layer = b.add(FIRE_LAYER, LayerKind::Additive);
        let mut authored_speed = HashMap::new();
        for c in clips {
            if c.root_distance > 0.5 && c.seconds > 0.0 {
                authored_speed.entry(c.name.clone()).or_insert(c.root_distance / c.seconds);
            }
        }
        let swing_jump_peak = b.by_name.get("swing_jump").and_then(|&i| clips[i].jump_peak).filter(|t| t.is_finite() && *t >= 0.0);
        Graph {
            nodes: b.nodes,
            clips: move_clips,
            lean_nodes,
            draw_layers,
            glide_lean,
            glove_hand,
            catch_layer,
            grenade_layers,
            windup_layers,
            aim_layers,
            throw_layer,
            fire_layer,
            authored_speed,
            swing_jump_peak,
        }
    }
}

/// What the sandbox's `Rig` keeps from frame to frame, and its Bevy player and transitions.
#[derive(Clone, Debug)]
struct Rig {
    player: Player,
    transitions: Transitions,
    current_clip: Option<&'static str>,
    prev_state: Option<MoveState>,
    land_timer: f32,
    /// The throw layer: how long it has played, and the throw count it has seen.
    throw_time: Option<f32>,
    throws_seen: Option<u32>,
    fire_time: Option<f32>,
    shots_seen: Option<u32>,
    /// The clip and time the sounds were last checked at.
    sound_clock: Option<(&'static str, f32)>,
    walking: bool,
    /// The run's lean: −1 (right) … 1 (left), how far its clips are in, and the yaw it was last
    /// measured at.
    lean: f32,
    lean_blend: f32,
    prev_yaw: Option<f32>,
    /// The wrench catch (upper body): how long it has played, and the catch count it has seen.
    catch_time: Option<f32>,
    catches_seen: Option<u32>,
    /// The grenade throw layer playing (see GRENADE_LAYERS) and for how long, and the throw
    /// count it has seen.
    grenade_time: Option<(usize, f32)>,
    grenades_seen: Option<u32>,
    /// The windup layer playing, with its weight.
    windup: Option<(usize, f32)>,
    /// How far the aim offsets are faded in.
    aim_weight: f32,
    /// The dash clip picked when the dash started (its direction relative to his facing).
    dash_clip: Option<&'static str>,
    /// How far the glove's hand pose is faded in.
    glove_weight: f32,
    /// The draw layer playing (grip, which, time), the weapon draws seen, and a draw waiting
    /// for a swing to end.
    draw: Option<(usize, usize, f32)>,
    equips_seen: Option<u32>,
    draw_pending: bool,
    /// The strafe jump picked as the jump (or double jump) started, and the strafe landing.
    jump_clip: Option<(MoveState, Option<&'static str>)>,
    strafe_landing: Option<&'static str>,
    /// Gliding: whether the intro is over, and the lean layer's weight and value (−1 right … 1
    /// left).
    glide_looping: bool,
    glide_lean_weight: f32,
    glide_lean_value: f32,
    /// How fast he's turning (rad/s, + to his left).
    turn_rate: f32,
    /// On a wall: whether the stick clip is over (then the slide loop plays).
    wall_looping: bool,
    /// Catching a ledge: the catch clip for how far he turns to face it; and the time left of
    /// the let-go clip after dropping off a caught ledge.
    ledge_catch: &'static str,
    ledge_drop_time: f32,
    /// On a swing: the clip it started with (picked as he fired), and the swing loop's Lean as
    /// the sim last gave it.
    swing_entry: Option<&'static str>,
    swing_lean: f32,
    idle_time: f32,
    fidgets_played: usize,
}

impl Rig {
    fn new(nodes: usize) -> Rig {
        Rig {
            player: Player::new(nodes),
            transitions: Transitions::default(),
            current_clip: None,
            prev_state: None,
            land_timer: 0.0,
            throw_time: None,
            throws_seen: None,
            fire_time: None,
            shots_seen: None,
            sound_clock: None,
            walking: false,
            lean: 0.0,
            lean_blend: 0.0,
            prev_yaw: None,
            catch_time: None,
            catches_seen: None,
            grenade_time: None,
            grenades_seen: None,
            windup: None,
            aim_weight: 0.0,
            dash_clip: None,
            glove_weight: 0.0,
            draw: None,
            equips_seen: None,
            draw_pending: false,
            jump_clip: None,
            strafe_landing: None,
            glide_looping: false,
            glide_lean_weight: 0.0,
            glide_lean_value: 0.0,
            turn_rate: 0.0,
            wall_looping: false,
            ledge_catch: "ledge_from_fall",
            ledge_drop_time: 0.0,
            swing_entry: None,
            swing_lean: 0.0,
            idle_time: 0.0,
            fidgets_played: 0,
        }
    }
}

/// Picks Ratchet's clips frame by frame as the sandbox does, and runs their crossfades and
/// clocks as Bevy does (see the crate docs).
#[derive(Clone, Debug)]
pub struct Animator {
    graph: Graph,
    rig: Rig,
    layers: Vec<Layer>,
    clock: Option<SoundClock>,
}

impl Animator {
    /// An animator for the host's clips, matched by name (the first, if two share one). Clips
    /// it doesn't look for are never played; ones it looks for and doesn't find are done
    /// without, as in the sandbox.
    pub fn new(clips: &[ClipInfo]) -> Self {
        let graph = Graph::new(clips);
        let rig = Rig::new(graph.nodes.len());
        Animator { graph, rig, layers: Vec::new(), clock: None }
    }

    /// Back to how `new` left it (after a load or a teleport): nothing playing, no clip picked,
    /// and the next update takes the host's counts in.
    pub fn reset(&mut self) {
        self.rig = Rig::new(self.graph.nodes.len());
        self.layers.clear();
        self.clock = None;
    }

    /// One frame, after that frame's sim steps: picks the clips, weighs and times them, and
    /// returns the layers to sample (also [`Animator::layers`]): the base layers in the order
    /// to blend them, then the additive ones in the order to add them.
    pub fn update(&mut self, sim: &Ratchet, input: &AnimInput) -> &[Layer] {
        // (A frame that takes no time, or makes no sense, moves nothing on.)
        let dt = if input.dt.is_finite() && input.dt > 0.0 { input.dt } else { 0.0 };
        let scale = if input.scale.is_finite() && input.scale > 0.0 { input.scale } else { sim.tuning().scale };
        let Animator { graph, rig, layers, clock } = self;
        layers.clear();
        // Respawned: no landing or let-go clip from where he was before.
        if input.respawned {
            rig.prev_state = None;
            rig.land_timer = 0.0;
            rig.ledge_drop_time = 0.0;
        }
        // No move clips: nothing plays (the sandbox poses him procedurally then).
        if graph.clips.is_empty() {
            *clock = None;
            return layers;
        }
        *clock = play_clips(graph, rig, sim, input, dt, scale);
        // Bevy's animation systems after the sandbox's: the transitions weigh the move clips,
        // every clip's clock moves on, the pose is sampled (the layers), and the clips that
        // have faded out stop.
        rig.transitions.advance(&mut rig.player, dt);
        rig.player.advance(dt, |node| graph.nodes[node].seconds);
        for additive in [false, true] {
            for (node, active) in rig.player.playing() {
                let n = &graph.nodes[node];
                // (Bevy leaves out a clip at no weight.)
                if let Some(clip) = n.clip
                    && (n.kind == LayerKind::Additive) == additive
                    && active.weight() != 0.0
                {
                    layers.push(Layer { clip, time: active.seek_time().clamp(0.0, n.seconds), weight: active.weight(), kind: n.kind });
                }
            }
        }
        rig.transitions.expire(&mut rig.player);
        layers
    }

    /// The layers from the last update.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// The move clip picked last: "swing_loop" for the Swingshot's blend of its five clips;
    /// None before the first update (or with no move clips).
    pub fn clip_name(&self) -> Option<&'static str> {
        self.rig.current_clip
    }

    /// That clip's index among the host's clips (None for the swing loop, which is a blend).
    pub fn clip(&self) -> Option<usize> {
        let node = *self.graph.clips.get(self.rig.current_clip?)?;
        self.graph.nodes[node].clip
    }

    /// The picked clip's clock from the last update, for the sounds it calls for (see
    /// [`SoundClock::passed`]). None when there's no clip to sound (the swing loop has none).
    pub fn sound_clock(&self) -> Option<SoundClock> {
        self.clock
    }
}

/// Whether one of the host's running counts moved since the last update: a new shot, throw,
/// draw or catch. The first update after `new` or `reset` only takes it in.
fn changed(seen: &mut Option<u32>, count: u32) -> bool {
    let changed = seen.is_some_and(|seen| seen != count);
    *seen = Some(count);
    changed
}

/// The sandbox's `Arsenal` as the clip choice reads it: there when he has weapons.
struct Arsenal {
    slot: Slot,
    aiming: bool,
    trigger: bool,
    stance: f32,
    cooking: bool,
    aim_target: Vec3,
}

/// Picks the clip for the current move and crossfades to it, and drives the layers (the
/// sandbox's `play_clips`). Returns the picked clip's clock for its sounds.
fn play_clips(g: &Graph, rig: &mut Rig, sim: &Ratchet, input: &AnimInput, dt: f32, scale: f32) -> Option<SoundClock> {
    let arsenal = input.slot.map(|slot| Arsenal {
        slot,
        aiming: input.aiming,
        trigger: input.trigger,
        stance: input.stance,
        cooking: input.cooking,
        aim_target: input.aim_target,
    });

    // The pistol's recoil layer: restarted with each shot.
    if let Some((node, duration)) = g.fire_layer {
        if changed(&mut rig.shots_seen, input.shots) {
            rig.fire_time = Some(0.0);
            rig.player.start(node);
        }
        if let Some(t) = rig.fire_time {
            let t = t + dt;
            if t >= duration {
                rig.player.stop(node);
                rig.fire_time = None;
            } else {
                let weight = (t / 0.03).min(1.0).min(((duration - t) / 0.12).max(0.0));
                if let Some(active) = rig.player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.fire_time = Some(t);
            }
        }
    }

    // Catching the wrench: the upper body reaches for it; the throw layer gives way.
    if let Some((node, duration)) = g.catch_layer {
        if changed(&mut rig.catches_seen, input.wrench_catches) {
            rig.catch_time = Some(0.0);
            rig.player.start(node);
            if let (Some(t), Some((_, throw_duration))) = (rig.throw_time, g.throw_layer) {
                rig.throw_time = Some(t.max(throw_duration - LAYER_FADE_OUT));
            }
        }
        if let Some(t) = rig.catch_time {
            let t = t + dt;
            if t >= duration {
                rig.player.stop(node);
                rig.catch_time = None;
            } else {
                let weight = CATCH_WEIGHT * (t / 0.05).min(1.0).min(((duration - t) / 0.12).max(0.0));
                if let Some(active) = rig.player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.catch_time = Some(t);
            }
        }
    }

    // A grenade throw: the whole-body one standing still, the upper-body one on the move or in
    // the air, layered over the move (the sandbox's weapons.rs times the release by the same
    // rule).
    let flat_speed = Vec2::new(input.velocity.x, input.velocity.z).length() / scale;
    let standing = sim.state() == MoveState::Ground && flat_speed <= 0.6;
    if changed(&mut rig.grenades_seen, input.grenade_throws) {
        let which = if standing { 0 } else { 1 };
        if let Some((node, _)) = g.grenade_layers[which] {
            if let Some((prev, _)) = rig.grenade_time.and_then(|(i, _)| g.grenade_layers[i]) {
                rig.player.stop(prev);
            }
            rig.player.start(node);
            rig.grenade_time = Some((which, 0.0));
        }
    }
    if let Some((which, t)) = rig.grenade_time
        && let Some((node, duration)) = g.grenade_layers[which]
    {
        let t = t + dt;
        if t >= duration {
            rig.player.stop(node);
            rig.grenade_time = None;
        } else {
            let weight = (t / LAYER_FADE_IN).min(1.0).min(((duration - t) / LAYER_FADE_OUT).max(0.0));
            if let Some(active) = rig.player.animation_mut(node) {
                active.set_weight(weight);
            }
            rig.grenade_time = Some((which, t));
        }
    }

    // Winding a grenade throw up while the fire button is held: the windup plays once and holds
    // its last pose; the throw takes over from it.
    let cooking = arsenal.as_ref().is_some_and(|a| a.cooking && a.slot == Slot::Shatterbomb);
    let releasing = rig.grenade_time.is_some_and(|(_, t)| t < 0.05);
    match rig.windup {
        None if cooking && !releasing => {
            let which = if standing { 0 } else { 1 };
            if let Some(node) = g.windup_layers[which] {
                rig.player.start(node).set_weight(0.0);
                rig.windup = Some((which, 0.0));
            }
        }
        Some((which, weight)) => {
            let on = cooking && !releasing;
            let weight = if on {
                (weight + dt / LAYER_FADE_IN).min(1.0)
            } else if releasing {
                0.0
            } else {
                weight - dt / 0.12
            };
            if let Some(node) = g.windup_layers[which] {
                if weight <= 0.0 {
                    rig.player.stop(node);
                    rig.windup = None;
                } else {
                    if let Some(active) = rig.player.animation_mut(node) {
                        active.set_weight(weight);
                    }
                    rig.windup = Some((which, weight));
                }
            }
        }
        None => {}
    }

    // Aim offsets while strafing (aiming, or a gun firing): the upper body bends toward what the
    // crosshair points at, by scrubbing the poses with the angle up/down and to the side of his
    // facing.
    // (Not while dashing: the dash clips are whole-body and the game puts the weapon away.)
    let throwing = input.wrench_throwing;
    let aiming_now = arsenal.as_ref().is_some_and(|a| a.aiming || (!throwing && (a.trigger || a.stance > 0.0)))
        && !matches!(sim.state(), MoveState::Dash | MoveState::AirDash);
    let fade = dt / LAYER_FADE_IN;
    rig.aim_weight = if aiming_now { (rig.aim_weight + fade).min(1.0) } else { (rig.aim_weight - fade).max(0.0) };
    let (pitch, side) = arsenal.as_ref().map_or((0.0, 0.0), |a| {
        let d = a.aim_target - (input.feet + Vec3::Y * 0.85 * scale);
        let yaw = d.x.atan2(d.z) - sim.facing_yaw();
        let side = (yaw + PI).rem_euclid(TAU) - PI;
        (d.y.atan2(Vec2::new(d.x, d.z).length()).to_degrees(), side.to_degrees())
    });
    let pistol_grip = !throwing && arsenal.as_ref().is_some_and(|a| a.slot == Slot::BurstPistol);
    let running = flat_speed > 0.6;
    let active = (if pistol_grip { 0 } else { 4 }) + if running { 2 } else { 0 };
    for (i, layer) in g.aim_layers.iter().enumerate() {
        let Some((node, duration)) = *layer else { continue };
        let weight = if i == active || i == active + 1 { rig.aim_weight } else { 0.0 };
        if weight <= 0.0 {
            if rig.player.animation(node).is_some() {
                rig.player.stop(node);
            }
            continue;
        }
        let half = duration / 2.0;
        let t = if i % 2 == 0 {
            if pitch >= 0.0 { half * (1.0 - pitch / AIM_UP) } else { half * (1.0 - pitch / AIM_DOWN) }
        } else {
            half * (1.0 + side / AIM_SIDE)
        };
        let anim = if rig.player.animation(node).is_some() { rig.player.animation_mut(node) } else { Some(rig.player.start(node).repeat()) };
        if let Some(anim) = anim {
            anim.set_speed(0.0).seek_to(t.clamp(0.0, duration * 0.999)).set_weight(weight);
        }
    }

    // The throw layer: restart it with each throw, fade it in and out over its length.
    if let Some((node, duration)) = g.throw_layer {
        if changed(&mut rig.throws_seen, input.wrench_throws) {
            rig.throw_time = Some(0.0);
            rig.player.start(node);
        }
        if let Some(t) = rig.throw_time {
            let t = t + dt;
            let weight = (t / LAYER_FADE_IN).min(1.0).min(((duration - t) / LAYER_FADE_OUT).max(0.0));
            if t >= duration {
                rig.player.stop(node);
                rig.throw_time = None;
            } else {
                if let Some(active) = rig.player.animation_mut(node) {
                    active.set_weight(weight);
                }
                rig.throw_time = Some(t);
            }
        }
    }
    let v = input.velocity / scale;
    let speed = Vec2::new(v.x, v.z).length();
    let state = sim.state();
    let airborne = |s: Option<MoveState>| {
        matches!(
            s,
            Some(MoveState::Jump | MoveState::DoubleJump | MoveState::Fall | MoveState::Glide | MoveState::AirDash | MoveState::WallJump | MoveState::LedgeJump)
        )
    };
    let landed = state == MoveState::Ground && airborne(rig.prev_state);
    // The game puts the weapon away on walls and ledges and draws it again after (not between
    // one of their moves and the next).
    let on_wall = |s: Option<MoveState>| s.is_some_and(holstered);
    if on_wall(rig.prev_state) && !on_wall(Some(state)) {
        rig.draw_pending = true;
    }
    // Catching a ledge: the catch clip turning round to it when he faced away (more than 135°;
    // the game blends these by the turn, ours picks one).
    if state == MoveState::LedgeHang && rig.prev_state != Some(MoveState::LedgeHang) && sim.ledge_move() == Some(LedgeMove::FromFall) {
        let turn = rig.prev_yaw.map_or(0.0, |prev| {
            let d = (sim.facing_yaw() - prev).rem_euclid(TAU);
            if d > PI { d - TAU } else { d }
        });
        rig.ledge_catch = if turn > 2.36 {
            "ledge_from_fall_l180"
        } else if turn < -2.36 {
            "ledge_from_fall_r180"
        } else {
            "ledge_from_fall"
        };
    }
    // Letting go of a caught ledge plays its let-go clip as he starts to fall (the host says so
    // for any of the frame's sim steps, since the event lasts one).
    let dropped = input.ledge_dropped;
    rig.ledge_drop_time = if dropped && state == MoveState::Fall {
        0.5
    } else if state == MoveState::Fall {
        (rig.ledge_drop_time - dt).max(0.0)
    } else {
        0.0
    };
    if landed {
        rig.land_timer = if speed > 0.6 { 0.35 } else { 0.3 };
    }
    rig.prev_state = Some(state);
    rig.land_timer = (rig.land_timer - dt).max(0.0);

    // Aiming (any weapon), or firing a gun and just after: strafing by direction, with the
    // pistol's one-handed set, or the wrench/glove set (standing with the glove, its own idle).
    // Swinging, and from a wrench throw until the catch, it's the melee grip whatever weapon is
    // out (a throw is the melee grip's "fire" in the game's animset).
    let melee_grip = throwing || matches!(state, MoveState::Melee | MoveState::Smash | MoveState::SmashLand);
    let pistol_out = !melee_grip && arsenal.as_ref().is_some_and(|a| a.slot == Slot::BurstPistol);
    let glove_out = !melee_grip && arsenal.as_ref().is_some_and(|a| a.slot == Slot::Shatterbomb);
    let carrying = arsenal.is_some() && !pistol_out;
    let gun_aim = arsenal.as_ref().is_some_and(|a| a.aiming || (!throwing && (a.trigger || a.stance > 0.0)));
    let (strafe_clips, aim_idle) = match (pistol_out, rig.walking) {
        (true, false) => (STRAFE_CLIPS, "aim_idle"),
        (true, true) => (STRAFE_WALK_CLIPS, "aim_idle"),
        (false, walking) => (if walking { WRENCH_STRAFE_WALK_CLIPS } else { WRENCH_STRAFE_CLIPS }, if glove_out { "glove_idle" } else { "wrench_aim_idle" }),
    };

    // Switching weapons draws the new one: the game's draw for its grip, whole-body standing,
    // upper-body moving. A switch during a swing draws once the swing is over.
    if changed(&mut rig.equips_seen, input.equips) {
        rig.draw_pending = true;
    }
    if rig.draw_pending && !melee_grip {
        rig.draw_pending = false;
        let grip = if pistol_out {
            0
        } else if glove_out {
            2
        } else {
            1
        };
        let which = if standing { 0 } else { 1 };
        if let Some((node, _)) = g.draw_layers[grip][which] {
            if let Some((prev, _)) = rig.draw.and_then(|(prev_grip, prev_which, _)| g.draw_layers[prev_grip][prev_which]) {
                rig.player.stop(prev);
            }
            rig.player.start(node).set_weight(0.0);
            rig.draw = Some((grip, which, 0.0));
        }
    }
    if let Some((grip, which, t)) = rig.draw
        && let Some((node, duration)) = g.draw_layers[grip][which]
    {
        let t = t + dt;
        if t >= duration {
            rig.player.stop(node);
            rig.draw = None;
        } else {
            if let Some(active) = rig.player.animation_mut(node) {
                active.set_weight((t / DRAW_FADE_IN).min(1.0).min(((duration - t) / DRAW_FADE_OUT).max(0.0)));
            }
            rig.draw = Some((grip, which, t));
        }
    }

    // The glove's hand pose while it's on.
    if let Some(node) = g.glove_hand {
        let fade = dt / 0.15;
        rig.glove_weight = if glove_out { (rig.glove_weight + fade).min(1.0) } else { (rig.glove_weight - fade).max(0.0) };
        if rig.glove_weight > 0.0 {
            let anim = if rig.player.animation(node).is_some() { rig.player.animation_mut(node) } else { Some(rig.player.start(node).repeat()) };
            if let Some(anim) = anim {
                anim.set_weight(GLOVE_HAND_WEIGHT * rig.glove_weight);
            }
        } else if rig.player.animation(node).is_some() {
            rig.player.stop(node);
        }
    }
    // Which of eight directions (clockwise from ahead) a ground-plane direction is, relative to
    // where he faces.
    let yaw = sim.facing_yaw();
    let (fwd, right) = (Vec2::new(yaw.sin(), yaw.cos()), Vec2::new(-yaw.cos(), yaw.sin()));
    let octant = |d: Vec2| {
        let angle = d.dot(right).atan2(d.dot(fwd));
        ((angle / FRAC_PI_4).round() as i32).rem_euclid(8) as usize
    };
    let strafe = strafe_clips[octant(Vec2::new(v.x, v.z))];
    // A dash keeps the clip for the direction it started in.
    if !matches!(state, MoveState::Dash | MoveState::AirDash) {
        rig.dash_clip = None;
    } else if rig.dash_clip.is_none() {
        let clips = if state == MoveState::Dash { DASH_CLIPS } else { AIR_DASH_CLIPS };
        let clip = clips[octant(sim.dash_dir())];
        rig.dash_clip = Some(if g.clips.contains_key(clip) { clip } else { clips[0] });
    }
    // Jumping while strafing: the clip for the jump's direction relative to his facing, picked as
    // each jump starts (the game's Strafe_Jump drivers), and the landing for how he comes down.
    let grip_set = if pistol_out { 0 } else { 1 };
    let quarter = |d: Vec2| -> usize {
        if d.length() < 0.6 {
            return 4;
        }
        ((d.dot(right).atan2(d.dot(fwd)) / FRAC_PI_2).round() as i32).rem_euclid(4) as usize
    };
    let flat = Vec2::new(v.x, v.z);
    if matches!(state, MoveState::Jump | MoveState::DoubleJump) {
        if rig.jump_clip.is_none_or(|(s, _)| s != state) {
            let set = if state == MoveState::Jump { STRAFE_JUMPS } else { STRAFE_DOUBLE_JUMPS };
            let clip = Some(set[grip_set][quarter(flat)]).filter(|c| gun_aim && g.clips.contains_key(c));
            rig.jump_clip = Some((state, clip));
        }
    } else {
        rig.jump_clip = None;
    }
    if landed {
        rig.strafe_landing = Some(STRAFE_LANDINGS[grip_set][quarter(flat)]).filter(|c| gun_aim && !rig.walking && g.clips.contains_key(c));
    } else if rig.land_timer <= 0.0 {
        rig.strafe_landing = None;
    }
    let strafe_jump = rig.jump_clip.and_then(|(_, c)| c);
    // Gliding under the heli-pack: the intro for his grip, then its loop.
    let (glide_intro, glide_loop) = if pistol_out {
        ("glide_intro_pistol", "glide_pistol")
    } else if glove_out {
        ("glide_intro_glove", "glove_glide")
    } else {
        ("glide_intro_wrench", "wrench_glide")
    };
    if state == MoveState::Glide {
        if rig.current_clip == Some(glide_intro) && g.clips.get(glide_intro).and_then(|&n| rig.player.animation(n)).is_some_and(|a| a.is_finished()) {
            rig.glide_looping = true;
        }
    } else {
        rig.glide_looping = false;
    }
    let heli_glide = g.clips.contains_key(glide_intro) && g.clips.contains_key(glide_loop);
    // On a wall: onto it (the stick clip), then the slide loop once that's over, still or
    // sliding, as the game does. Left or right is the sim's pick (by the camera, as in the game).
    let (wall_stick, wall_slide, wall_jump) = if sim.wall_right() {
        ("wall_stick_right", "wall_slide_right", "wall_jump_right")
    } else {
        ("wall_stick_left", "wall_slide_left", "wall_jump_left")
    };
    if state != MoveState::WallStick {
        rig.wall_looping = false;
    } else if rig.current_clip == Some(wall_stick) && g.clips.get(wall_stick).and_then(|&n| rig.player.animation(n)).is_some_and(|a| a.is_finished()) {
        rig.wall_looping = true;
    }
    // A swing starts with the pull in when he fires near the ground, else the intro, picked as
    // he fires.
    if state != MoveState::Swing {
        rig.swing_entry = None;
    } else if rig.swing_entry.is_none() {
        let low = input.ground_distance.is_some_and(|d| d < 0.9 * scale);
        rig.swing_entry = Some(if low { "swing_pull" } else { "swing_intro" });
    }
    let want: &'static str = match state {
        MoveState::Ground if rig.land_timer > 0.0 && rig.strafe_landing.is_some() => rig.strafe_landing.unwrap_or("land"),
        MoveState::Ground if gun_aim && speed > 0.6 => strafe,
        MoveState::Ground if gun_aim => aim_idle,
        MoveState::Ground if pistol_out && speed > 0.6 => "pistol_run",
        MoveState::Ground if pistol_out && rig.land_timer <= 0.0 => "pistol_idle",
        MoveState::Ground if rig.land_timer > 0.0 && speed > WALK_TO_RUN => "land_run",
        MoveState::Ground if rig.land_timer > 0.0 && speed > 0.6 => "land_walk",
        MoveState::Ground if speed > 0.6 && rig.walking => "walk",
        MoveState::Ground if speed > 0.6 => "run",
        MoveState::Ground if rig.land_timer > 0.0 => "land",
        MoveState::Ground if rig.idle_time > FIDGET_AFTER => FIDGETS[rig.fidgets_played % FIDGETS.len()],
        MoveState::Ground => "idle",
        MoveState::Jump | MoveState::DoubleJump if strafe_jump.is_some() => strafe_jump.unwrap_or("jump"),
        MoveState::Jump if v.y < -5.0 => "fall",
        MoveState::Jump => "jump",
        MoveState::DoubleJump => "double_jump",
        MoveState::Fall if rig.ledge_drop_time > 0.0 => "ledge_drop",
        MoveState::Fall => "fall",
        MoveState::Glide if heli_glide && rig.glide_looping => glide_loop,
        MoveState::Glide if heli_glide => glide_intro,
        MoveState::Glide => "glide",
        MoveState::Dash | MoveState::AirDash => rig.dash_clip.unwrap_or("dash"),
        MoveState::Melee => MELEE_CLIPS[sim.melee_swing().unwrap_or(0).min(MELEE_CLIPS.len() - 1)],
        MoveState::Smash => "smash_intro",
        MoveState::SmashLand => "smash",
        MoveState::WallStick if rig.wall_looping => wall_slide,
        MoveState::WallStick => wall_stick,
        MoveState::WallJump => wall_jump,
        MoveState::LedgeHang => match sim.ledge_move() {
            Some(LedgeMove::Grab) => "ledge_grab",
            Some(LedgeMove::FromFall) => rig.ledge_catch,
            Some(LedgeMove::UpTo) => "ledge_up",
            Some(LedgeMove::DownTo) => "ledge_down_to",
            None => "ledge_idle",
        },
        // The climb up a caught ledge (Ledge_Jump_Up_InAir), the let-go with a turn, and the
        // ledge jump, which the game animates with the double jump's flip (Jump_In_Air).
        MoveState::LedgeClimb => "ledge_jump",
        MoveState::LedgeDrop => "ledge_drop_turn",
        MoveState::LedgeJump => "double_jump",
        // The Swingshot (RiftApart.exe's animation drivers): the swing's first clip through the
        // swing in and on past the target; from the first swing back, the loop; the jump off;
        // the Hookshot's reel.
        MoveState::Swing if sim.swing_phase().is_some_and(|phase| phase >= 2) => SWING_LOOP,
        MoveState::Swing => rig.swing_entry.unwrap_or("swing_intro"),
        MoveState::Hook => "hook_reel",
        MoveState::SwingJump => "swing_jump",
    };
    let want = match carried(want) {
        c if carrying && g.clips.contains_key(c) => c,
        _ => want,
    };
    let want = if g.clips.contains_key(want) {
        want
    } else if want == "walk" || want.starts_with("land_") {
        if speed > 0.6 { "run" } else { "land" }
    } else {
        "idle"
    };
    if rig.current_clip != Some(want)
        && let Some(&node) = g.clips.get(want)
    {
        let fade = fade_ms(rig.current_clip, want);
        let active = rig.transitions.play(&mut rig.player, node, Duration::from_millis(fade));
        if LOOPING.contains(&want) {
            active.repeat();
        }
        // The transitions weigh it later this frame, and its clips follow its weight a frame
        // behind: they start from none.
        if want == SWING_LOOP {
            active.set_weight(0.0);
        }
        rig.current_clip = Some(want);
    }
    blend_swing_loop(g, rig, sim.swing_lean_speed().map(|(lean, _)| lean));
    // The sounds the clip calls for, as its time passes them (looping clips wrap around). A
    // clip that just started sounds what it passed already.
    let mut clock = None;
    if let Some(&node) = g.clips.get(want)
        && let Some(now) = rig.player.animation(node).map(|a| a.seek_time())
    {
        let from = match rig.sound_clock {
            Some((clip, t)) if clip == want => Some(t),
            _ => None,
        };
        rig.sound_clock = Some((want, now));
        clock = g.nodes[node].clip.map(|clip| SoundClock { clip, from, now });
    }

    // Match the walk, run and strafe cycles (and the moving landings) to the actual speed: at
    // the speed a clip was authored for, it plays at 1x.
    if (SPEED_MATCHED.contains(&want) || (want.contains("strafe") && !want.contains("jump") && !want.contains("land")))
        && let Some(&node) = g.clips.get(want)
    {
        let tuning = sim.tuning();
        let authored = g
            .authored_speed
            .get(want)
            .copied()
            .unwrap_or_else(|| (if want.contains("strafe_walk") { tuning.aim_speeds.run_max } else { tuning.ground_speeds.run_max }) / scale);
        if let Some(active) = rig.player.animation_mut(node) {
            let range = if want.contains("land") { (0.7, 1.3) } else { (0.5, 1.5) };
            active.set_speed((speed / authored.max(0.1)).clamp(range.0, range.1));
        }
    }
    // The wall jump's in-air clip plays at a quarter speed, the slowest the game plays a jump
    // clip: it speeds it up only to finish as he lands, and between walls it never does, so the
    // 0.2 s clip lasts the flight (RiftApart.exe). Past its end it holds its last pose.
    if want == wall_jump
        && let Some(&node) = g.clips.get(want)
        && let Some(active) = rig.player.animation_mut(node)
    {
        active.set_speed(WALL_JUMP_CLIP_RATE);
    }
    // The swing jump's flip plays so that its JumpPeak event comes at the jump's top, where the
    // fall takes over: 0.531 s into the clip at 0.5 s, about 1.06x. (Assumption: the game's
    // driver times it this way; its rate wasn't read.)
    if want == "swing_jump"
        && let (Some(&node), Some(peak)) = (g.clips.get(want), g.swing_jump_peak)
        && let Some(active) = rig.player.animation_mut(node)
    {
        let jump = &sim.tuning().swingshot.jump;
        let to_top = (2.0 * jump.height_min / jump.gravity()).sqrt();
        active.set_speed(peak / to_top.max(0.05));
    }
    rig.walking = if rig.walking { speed < WALK_TO_RUN } else { speed < RUN_TO_WALK };
    lean_into_turns(g, rig, sim.facing_yaw(), want, dt);
    // The glide leans into turns: its lean poses scrubbed by the turn rate.
    if let Some((node, duration)) = g.glide_lean {
        let gliding = state == MoveState::Glide;
        let target = if gliding { (rig.turn_rate / FULL_LEAN_TURN_RATE).clamp(-1.0, 1.0) } else { 0.0 };
        rig.glide_lean_value += (target - rig.glide_lean_value) * (1.0 - (-dt / 0.2).exp());
        rig.glide_lean_weight = if gliding { (rig.glide_lean_weight + dt / 0.25).min(1.0) } else { (rig.glide_lean_weight - dt / 0.25).max(0.0) };
        if rig.glide_lean_weight > 0.0 {
            let t = (duration / 2.0 * (1.0 - rig.glide_lean_value)).clamp(0.0, duration * 0.999);
            let anim = if rig.player.animation(node).is_some() { rig.player.animation_mut(node) } else { Some(rig.player.start(node).repeat()) };
            if let Some(anim) = anim {
                anim.set_speed(0.0).seek_to(t).set_weight(rig.glide_lean_weight);
            }
        } else if rig.player.animation(node).is_some() {
            rig.player.stop(node);
        }
    }
    // Fidgets play once, then it's back to the idle (and a while until the next one).
    let idling = matches!(want, "idle" | "wrench_idle");
    rig.idle_time = if idling { rig.idle_time + dt } else { rig.idle_time };
    if FIDGETS.contains(&want)
        && let Some(&node) = g.clips.get(want)
        && rig.player.animation(node).is_none_or(|a| a.is_finished())
    {
        rig.idle_time = 0.0;
        rig.fidgets_played += 1;
    } else if !FIDGETS.contains(&want) && !idling {
        rig.idle_time = 0.0;
    }
    clock
}

/// Blends the run's lean-left and lean-right clips in by how fast he turns, kept in step with
/// the run (same length, same time), and fades them out when he stops running. `clip` is the
/// move clip playing. It measures how fast he turns for the glide's lean and the ledge catch
/// too, which the sandbox does only when it has the lean clips.
fn lean_into_turns(g: &Graph, rig: &mut Rig, yaw: f32, clip: &str, dt: f32) {
    let rate = rig.prev_yaw.map_or(0.0, |prev| {
        let mut d = yaw - prev;
        // (Only a broken yaw jumps this far; it would never wrap.)
        if d.abs() > 2.0 * TAU {
            d = d.rem_euclid(TAU);
        }
        while d > PI {
            d -= TAU;
        }
        while d < -PI {
            d += TAU;
        }
        d / dt.max(1e-4)
    });
    rig.prev_yaw = Some(yaw);
    rig.turn_rate = rate;
    if g.lean_nodes.is_empty() {
        return;
    }
    let running = g.lean_nodes.iter().any(|(run, _)| *run == clip);
    // Turning toward +yaw is to his left.
    let target = if running { (rate / FULL_LEAN_TURN_RATE).clamp(-1.0, 1.0) } else { 0.0 };
    rig.lean += (target - rig.lean) * (1.0 - (-dt / 0.15).exp());
    rig.lean_blend += ((running as u8 as f32) - rig.lean_blend) * (1.0 - (-dt / 0.08).exp());
    let weights = [rig.lean.max(0.0), (-rig.lean).max(0.0)];
    for &(run_clip, nodes) in &g.lean_nodes {
        // Each run's leans follow it, also while it fades out.
        let run = g.clips.get(run_clip).and_then(|&n| rig.player.animation(n)).map(|a| (a.seek_time(), a.speed(), a.weight()));
        for (node, w) in nodes.into_iter().zip(weights) {
            // The transitions hold the run's own weight at 1, so a lean weighted w/(1−w) takes
            // its share w of the blend.
            let w = w * rig.lean_blend;
            let w = w.min(0.95) / (1.0 - w.min(0.95));
            match run {
                Some((time, speed, run_weight)) if w > 1e-3 => {
                    if rig.player.animation(node).is_none() {
                        rig.player.start(node).repeat();
                    }
                    if let Some(active) = rig.player.animation_mut(node) {
                        active.seek_to(time).set_speed(speed).set_weight(w * run_weight);
                    }
                }
                _ => rig.player.stop(node),
            }
        }
    }
}

/// The swing loop: while its node plays (faded in and out by the transitions like a move clip,
/// see `Graph::new`), all its clips play under it from when it starts, never restarted, each
/// taking its share of the node's weight by the swing's Lean; they stop with it. `lean` is the
/// sim's while he swings; after he lets go, the loop fades out as it was. (The game also sets a
/// Speed on the loop, rising as he passes slowly under the target. It isn't used: the sim already
/// pulls Lean toward the stall by as much, and what else the game's graph does with it isn't
/// decoded.)
fn blend_swing_loop(g: &Graph, rig: &mut Rig, lean: Option<f32>) {
    let Some(&carrier) = g.clips.get(SWING_LOOP) else { return };
    if let Some(lean) = lean {
        rig.swing_lean = lean;
    }
    let weight = rig.player.animation(carrier).map(|a| a.weight());
    for ((_, name), share) in SWING_LOOP_CLIPS.into_iter().zip(swing_loop_weights(rig.swing_lean)) {
        let Some(&node) = g.clips.get(name) else { continue };
        match weight {
            Some(weight) => {
                if rig.player.animation(node).is_none() {
                    rig.player.start(node).repeat();
                }
                if let Some(active) = rig.player.animation_mut(node) {
                    active.set_weight(share * weight);
                }
            }
            None => rig.player.stop(node),
        }
    }
}

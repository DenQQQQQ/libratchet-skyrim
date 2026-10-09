//! Bevy 0.19's clip clocks and crossfades, without Bevy: `ActiveAnimation`, `AnimationPlayer`
//! and `AnimationTransitions` (bevy_animation's `lib.rs` and `transition.rs`), as the sandbox's
//! animation graph runs them. Nodes are the graph's clip nodes, by index.
//!
//! Each frame Bevy runs, in this order: the sandbox's own systems (which start, stop, weigh and
//! seek clips), then `advance_transitions` ([`Transitions::advance`]), `advance_animations`
//! ([`Player::advance`]), the pose (sampled at each clip's time and weight), and
//! `expire_completed_transitions` ([`Transitions::expire`]).

use std::time::Duration;

/// A clip playing (Bevy's `ActiveAnimation`): its weight, rate and time. Bevy's pause, repeat
/// counts and events aren't used by the sandbox and are left out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Active {
    weight: f32,
    /// Bevy's `RepeatAnimation::Forever` (true) or `Never`.
    repeat: bool,
    speed: f32,
    /// Seconds into the clip: wrapped for a looping clip; a one-shot stops where it passed its
    /// end (sampling holds its last pose there).
    seek_time: f32,
    completions: u32,
}

impl Default for Active {
    fn default() -> Self {
        Active { weight: 1.0, repeat: false, speed: 1.0, seek_time: 0.0, completions: 0 }
    }
}

impl Active {
    /// A one-shot that has played to its end. A looping clip never finishes.
    pub(crate) fn is_finished(&self) -> bool {
        !self.repeat && self.completions >= 1
    }

    /// Moves the clock on by `delta` at its speed (Bevy's `ActiveAnimation::update`).
    fn update(&mut self, delta: f32, duration: f32) {
        if self.is_finished() {
            return;
        }
        self.seek_time += delta * self.speed;
        let over_time = self.speed > 0.0 && self.seek_time >= duration;
        let under_time = self.speed < 0.0 && self.seek_time < 0.0;
        if over_time || under_time {
            self.completions += 1;
            if self.is_finished() {
                return;
            }
        }
        // (Bevy wraps by the clip's length; a clip with none would make that NaN, so it stays at 0.)
        if self.seek_time >= duration {
            self.seek_time = if duration > 0.0 { self.seek_time % duration } else { 0.0 };
        }
        if self.seek_time < 0.0 {
            self.seek_time += duration;
        }
    }

    /// Back to the start, as if no time had passed (Bevy's `replay`). Weight, speed and repeat
    /// stay as they were.
    fn replay(&mut self) {
        self.completions = 0;
        self.seek_time = 0.0;
    }

    pub(crate) fn weight(&self) -> f32 {
        self.weight
    }

    pub(crate) fn set_weight(&mut self, weight: f32) -> &mut Self {
        self.weight = weight;
        self
    }

    pub(crate) fn speed(&self) -> f32 {
        self.speed
    }

    pub(crate) fn set_speed(&mut self, speed: f32) -> &mut Self {
        self.speed = speed;
        self
    }

    pub(crate) fn seek_time(&self) -> f32 {
        self.seek_time
    }

    pub(crate) fn seek_to(&mut self, seek_time: f32) -> &mut Self {
        self.seek_time = seek_time;
        self
    }

    /// Loops forever.
    pub(crate) fn repeat(&mut self) -> &mut Self {
        self.repeat = true;
        self
    }
}

/// The clips playing (Bevy's `AnimationPlayer`), by node.
#[derive(Clone, Debug)]
pub(crate) struct Player {
    active: Vec<Option<Active>>,
}

impl Player {
    pub(crate) fn new(nodes: usize) -> Self {
        Player { active: vec![None; nodes] }
    }

    /// Plays a node's clip from its start, restarting it if it was playing.
    pub(crate) fn start(&mut self, node: usize) -> &mut Active {
        let active = self.active[node].get_or_insert_with(Active::default);
        active.replay();
        active
    }

    pub(crate) fn stop(&mut self, node: usize) {
        self.active[node] = None;
    }

    pub(crate) fn animation(&self, node: usize) -> Option<&Active> {
        self.active[node].as_ref()
    }

    pub(crate) fn animation_mut(&mut self, node: usize) -> Option<&mut Active> {
        self.active[node].as_mut()
    }

    /// Every clip playing moves on by `dt` (Bevy's `advance_animations`), `duration` giving each
    /// node's clip length.
    pub(crate) fn advance(&mut self, dt: f32, duration: impl Fn(usize) -> f32) {
        for (node, active) in self.active.iter_mut().enumerate() {
            if let Some(active) = active {
                active.update(dt, duration(node));
            }
        }
    }

    /// The clips playing, in node order.
    pub(crate) fn playing(&self) -> impl Iterator<Item = (usize, &Active)> {
        self.active.iter().enumerate().filter_map(|(node, a)| a.as_ref().map(|a| (node, a)))
    }
}

/// A clip fading out: its weight before the newer ones take theirs, and how fast that drops.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fading {
    weight: f32,
    decline_per_sec: f32,
    node: usize,
}

/// The crossfades between move clips (Bevy's `AnimationTransitions`): the main clip, and the
/// ones fading out, oldest first.
#[derive(Clone, Debug, Default)]
pub(crate) struct Transitions {
    main: Option<usize>,
    fading: Vec<Fading>,
}

impl Transitions {
    /// Plays `node` from its start as the main clip, fading the one playing out over `fade` (a
    /// zero fade cuts). A clip that was fading out stops fading: it's the main clip again.
    pub(crate) fn play<'p>(&mut self, player: &'p mut Player, node: usize, fade: Duration) -> &'p mut Active {
        if let Some(old) = self.main.replace(node)
            && let Some(old_animation) = player.animation(old)
        {
            self.fading.push(Fading { weight: old_animation.weight, decline_per_sec: 1.0 / fade.as_secs_f32(), node: old });
        }
        self.fading.retain(|f| f.node != node);
        player.start(node)
    }

    /// Weighs the clips (Bevy's `advance_transitions`, a "greedy layer" system): the newest
    /// fading clip takes as much as it still has, each older one that share of what's left, and
    /// the main clip the rest, so they sum to 1.
    pub(crate) fn advance(&mut self, player: &mut Player, dt: f32) {
        let mut remaining = 1.0;
        for f in self.fading.iter_mut().rev() {
            f.weight = (f.weight - f.decline_per_sec * dt).max(0.0);
            let Some(animation) = player.animation_mut(f.node) else { continue };
            animation.weight = f.weight * remaining;
            remaining -= animation.weight;
        }
        if let Some(main) = self.main
            && let Some(animation) = player.animation_mut(main)
        {
            animation.weight = remaining;
        }
    }

    /// Stops the clips that have faded all the way out (Bevy's `expire_completed_transitions`).
    pub(crate) fn expire(&mut self, player: &mut Player) {
        self.fading.retain(|f| {
            let expire = f.weight <= 0.0;
            if expire {
                player.stop(f.node);
            }
            !expire
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.02;

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn weight(player: &Player, node: usize) -> Option<f32> {
        player.animation(node).map(|a| a.weight())
    }

    /// One frame after the sandbox's systems: weights, then clocks, then expired fades.
    fn frame(transitions: &mut Transitions, player: &mut Player) {
        transitions.advance(player, DT);
        player.advance(DT, |_| 1.0);
        transitions.expire(player);
    }

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-5)
    }

    #[test]
    fn a_new_clip_fades_in_as_the_old_one_fades_out() {
        let (mut player, mut transitions) = (Player::new(3), Transitions::default());
        transitions.play(&mut player, 0, ms(160));
        frame(&mut transitions, &mut player);
        assert!(close(weight(&player, 0), 1.0), "nothing to fade from");
        transitions.play(&mut player, 1, ms(100));
        transitions.advance(&mut player, DT);
        assert!(close(weight(&player, 0), 0.8) && close(weight(&player, 1), 0.2));
        player.advance(DT, |_| 1.0);
        transitions.expire(&mut player);
        // Within five frames more it's gone.
        for _ in 0..5 {
            frame(&mut transitions, &mut player);
        }
        assert_eq!(weight(&player, 0), None, "the old clip stops once faded out");
        assert!(close(weight(&player, 1), 1.0));
    }

    #[test]
    fn the_newest_fading_clip_takes_its_share_first() {
        let (mut player, mut transitions) = (Player::new(3), Transitions::default());
        transitions.play(&mut player, 0, ms(160));
        frame(&mut transitions, &mut player);
        transitions.play(&mut player, 1, ms(100));
        frame(&mut transitions, &mut player);
        // 0 at 0.8, 1 at 0.2; now 2 comes in over 0.2 s: 1 fades from 0.2, 0 keeps on fading.
        transitions.play(&mut player, 2, ms(200));
        transitions.advance(&mut player, DT);
        let w1 = 0.2 - DT / 0.2;
        let w0 = (0.8 - DT / 0.1) * (1.0 - w1);
        assert!(close(weight(&player, 1), w1), "{:?}", weight(&player, 1));
        assert!(close(weight(&player, 0), w0), "{:?}", weight(&player, 0));
        assert!(close(weight(&player, 2), 1.0 - w1 - w0));
    }

    #[test]
    fn a_zero_fade_cuts() {
        let (mut player, mut transitions) = (Player::new(2), Transitions::default());
        transitions.play(&mut player, 0, ms(160));
        frame(&mut transitions, &mut player);
        transitions.play(&mut player, 1, ms(0));
        transitions.advance(&mut player, DT);
        assert!(close(weight(&player, 0), 0.0) && close(weight(&player, 1), 1.0));
        transitions.expire(&mut player);
        assert_eq!(weight(&player, 0), None);
        // Even on a frame that takes no time.
        transitions.play(&mut player, 0, ms(0));
        transitions.advance(&mut player, 0.0);
        assert!(close(weight(&player, 1), 0.0) && close(weight(&player, 0), 1.0));
    }

    #[test]
    fn playing_a_fading_clip_again_takes_it_back_from_its_start() {
        let (mut player, mut transitions) = (Player::new(2), Transitions::default());
        transitions.play(&mut player, 0, ms(160)).repeat().set_speed(1.5);
        frame(&mut transitions, &mut player);
        transitions.play(&mut player, 1, ms(160));
        frame(&mut transitions, &mut player);
        // 0 fading at 1 - 0.125, 1 coming in at 0.125.
        let active = transitions.play(&mut player, 0, ms(320));
        // From its start, still looping at its rate; its weight is the transitions' next frame.
        assert_eq!(active.seek_time(), 0.0);
        assert!(active.repeat && active.speed() == 1.5);
        transitions.advance(&mut player, DT);
        assert_eq!(transitions.fading.len(), 1, "only the other clip fades");
        // 1 fades out from the 0.125 it had; 0 has the rest.
        let w1 = 0.125 - DT / 0.32;
        assert!(close(weight(&player, 1), w1) && close(weight(&player, 0), 1.0 - w1), "{:?} {:?}", weight(&player, 1), weight(&player, 0));
    }

    #[test]
    fn loops_wrap_and_one_shots_hold_at_their_end() {
        let mut player = Player::new(2);
        player.start(0).repeat();
        player.start(1);
        for _ in 0..60 {
            player.advance(DT, |_| 1.0);
        }
        // 1.2 s into a 1 s clip.
        assert!((player.animation(0).unwrap().seek_time() - 0.2).abs() < 1e-4);
        assert!(!player.animation(0).unwrap().is_finished());
        let one_shot = player.animation(1).unwrap();
        assert!(one_shot.is_finished() && one_shot.seek_time() >= 1.0 && one_shot.seek_time() < 1.0 + DT + 1e-5);
        // Speed scales the clock; a held clip doesn't move.
        player.start(0).set_speed(0.5);
        player.advance(0.1, |_| 1.0);
        assert!((player.animation(0).unwrap().seek_time() - 0.05).abs() < 1e-6);
        player.animation_mut(0).unwrap().set_speed(0.0).seek_to(0.7);
        player.advance(0.1, |_| 1.0);
        assert_eq!(player.animation(0).unwrap().seek_time(), 0.7);
    }
}

//! The game's own sounds, when `riftapart-assets/sounds` exists (written by
//! tools/extract_riftapart_sounds.py from the user's install): every gameplay moment plays one
//! random variation of each of its layers, with the layer's delay, pitch range and volume.
//! Systems ask for a sound with a `SoundCue`, naming a moment (a key of sounds.json's
//! "moments") or a Wwise event (as the animation clips call them; sounds.json maps those to
//! moments). Without the files the sandbox stays silent.

use crate::{Player, Settings};
use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use std::collections::HashMap;

/// Overall loudness (ours).
const MASTER: f32 = 0.7;
/// The game's per-sound mix levels span about 45 dB before its own buses and loudness mixing,
/// which we don't have; we play them at half their difference from this level, within a range
/// (ours).
const LEVEL_REFERENCE: f32 = 24.0;
const LEVEL_SCALE: f32 = 0.5;
const LEVEL_RANGE: (f32, f32) = (-18.0, 3.0);

/// A sound to play: a moment or a Wwise event name, where it happens (world sounds fade with
/// distance from Ratchet), for loops whether it starts or stops, and what it belongs to.
#[derive(Message, Clone, Debug)]
pub struct SoundCue {
    pub name: String,
    pub at: Option<Vec3>,
    pub looping: Option<bool>,
    pub owner: Option<Entity>,
}

impl SoundCue {
    /// A one-shot on Ratchet.
    pub fn play(name: impl Into<String>) -> Self {
        SoundCue { name: name.into(), at: None, looping: None, owner: None }
    }

    /// A one-shot somewhere in the world.
    pub fn at(name: impl Into<String>, at: Vec3) -> Self {
        SoundCue { name: name.into(), at: Some(at), looping: None, owner: None }
    }

    /// Starts a loop on Ratchet (once; a second start does nothing).
    pub fn start(name: impl Into<String>) -> Self {
        SoundCue { name: name.into(), at: None, looping: Some(true), owner: None }
    }

    pub fn stop(name: impl Into<String>) -> Self {
        SoundCue { name: name.into(), at: None, looping: Some(false), owner: None }
    }

    /// A moment that belongs to `owner` (a grenade in flight): its loop layers loop, and all
    /// it plays, sounds still waiting on their delay too, stops when the owner is gone
    /// (RiftApart.exe plays the Shatterbomb's flight on the grenade, and its explosion stops
    /// it). Each owner's sounds are its own, so two grenades in flight both sound.
    pub fn on(name: impl Into<String>, owner: Entity) -> Self {
        SoundCue { name: name.into(), at: None, looping: None, owner: Some(owner) }
    }
}

struct Layer {
    files: Vec<Handle<AudioSource>>,
    /// Per file (the layer's own volume is folded in).
    volumes: Vec<f32>,
    delay: f32,
    /// Pitch range in cents.
    pitch: (f32, f32),
    looped: bool,
    /// A looping layer that plays a fresh file this often instead of looping one (seconds).
    interval: Option<f32>,
}

struct Moment {
    layers: Vec<Layer>,
    /// How often it plays at all (the game's random containers can pick silence).
    chance: f32,
    gain: f32,
}

#[derive(Resource, Default)]
pub struct Sounds {
    moments: HashMap<String, Moment>,
    /// Wwise event name → moment.
    events: HashMap<String, String>,
    rng: u32,
}

impl Sounds {
    pub fn available(&self) -> bool {
        !self.moments.is_empty()
    }

    fn next(&mut self) -> u32 {
        // xorshift32
        let mut x = self.rng.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// One file of a layer, its volume and playback speed (for its pitch).
    fn pick(&mut self, moment: &str, layer: usize) -> Option<(Handle<AudioSource>, f32, f32)> {
        let (n, pitch) = {
            let l = &self.moments.get(moment)?.layers[layer];
            (l.files.len(), l.pitch)
        };
        let i = self.next() as usize % n.max(1);
        let cents = pitch.0 + (pitch.1 - pitch.0) * self.unit();
        let m = &self.moments[moment];
        let l = &m.layers[layer];
        Some((l.files.get(i)?.clone(), l.volumes.get(i).copied().unwrap_or(1.0) * m.gain, 2f32.powf(cents / 1200.0)))
    }
}

/// While a demo records: every sound it plays, one line each ("seconds<TAB>file<TAB>volume<TAB>
/// speed<TAB>once|loop[<TAB>moment]", or "seconds<TAB>moment<TAB>0<TAB>1<TAB>stop"), so
/// tools/mix_demo_audio.py can lay the soundtrack under the recorded frames.
#[derive(Resource)]
pub struct SoundLog(pub std::path::PathBuf);

/// A playing loop (a looping file, or a retrigger driver), so its stop cue finds it.
#[derive(Component)]
struct LoopOf(String);

/// A sound (playing, delayed or retriggering) of a moment played on an entity: it stops when
/// that entity is gone. The key names them in the recording's log.
#[derive(Component, Clone)]
struct OwnedBy(Entity, String);

/// What a sound is tagged with: a loop its stop cue finds, or an owner.
#[derive(Clone)]
enum Tag {
    None,
    Loop(String),
    Owned(OwnedBy),
}

/// A layer sound that starts after its delay (looping, for a loop layer played on an owner).
#[derive(Component)]
struct Delayed {
    left: f32,
    file: Handle<AudioSource>,
    volume: f32,
    speed: f32,
    looped: bool,
    tag: Tag,
}

/// Plays a fresh file of a moment's layer every `every` seconds while its loop runs.
#[derive(Component)]
struct Retrigger {
    moment: String,
    layer: usize,
    every: f32,
    left: f32,
    tag: Tag,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Sounds>().add_message::<SoundCue>().add_systems(Startup, load).add_systems(Update, (play, delayed, retrigger, stop_orphans).chain());
}

fn load(settings: Res<Settings>, assets: Res<AssetServer>, mut sounds: ResMut<Sounds>) {
    let Some(dir) = &settings.model_dir else { return };
    let manifest = dir.join("sounds").join("sounds.json");
    let Ok(text) = std::fs::read_to_string(&manifest) else {
        info!("no {} (see tools/extract_riftapart_sounds.py): no sounds", manifest.display());
        return;
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            warn!("{}: {e}", manifest.display());
            return;
        }
    };
    let f32_of = |v: &serde_json::Value, default: f32| v.as_f64().map_or(default, |x| x as f32);
    let mut files = 0;
    for (name, m) in v["moments"].as_object().into_iter().flatten() {
        let layers: Vec<Layer> = m["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| {
                let paths: Vec<&str> = l["files"].as_array().into_iter().flatten().filter_map(|f| f.as_str()).collect();
                let volume = f32_of(&l["volume"], 1.0);
                let volumes = (0..paths.len()).map(|i| volume * f32_of(&l["volumes"][i], 1.0)).collect();
                let pitch = (f32_of(&l["pitch"][0], 0.0), f32_of(&l["pitch"][1], 0.0));
                Layer {
                    files: paths.iter().map(|f| assets.load(format!("sounds/{f}"))).collect(),
                    volumes,
                    delay: f32_of(&l["delay"], 0.0),
                    pitch,
                    looped: l["loop"].as_bool().unwrap_or(false),
                    interval: l["interval"].as_f64().map(|x| x as f32).filter(|x| *x > 0.0),
                }
            })
            .filter(|l| !l.files.is_empty())
            .collect();
        files += layers.iter().map(|l| l.files.len()).sum::<usize>();
        let level = f32_of(&m["level_db"], LEVEL_REFERENCE);
        let gain_db = ((level - LEVEL_REFERENCE) * LEVEL_SCALE).clamp(LEVEL_RANGE.0, LEVEL_RANGE.1);
        let moment = Moment { layers, chance: f32_of(&m["chance"], 1.0), gain: 10f32.powf(gain_db / 20.0) };
        sounds.moments.insert(name.clone(), moment);
    }
    for (event, moment) in v["events"].as_object().into_iter().flatten() {
        if let Some(moment) = moment.as_str() {
            sounds.events.insert(event.clone(), moment.to_string());
        }
    }
    sounds.rng = 0x2545_F491;
    info!("sounds: {} moments, {files} files, {} clip events", sounds.moments.len(), sounds.events.len());
}

/// How loud a world sound is `distance` metres (Ratchet-sized) from Ratchet (ours: full up to
/// 6 m, then fading).
fn falloff(distance: f32) -> f32 {
    if distance <= 6.0 { 1.0 } else { (6.0 / distance).powf(0.85) }
}

/// Starts one sound now, and logs it when a demo records (an owned one with its owner's key,
/// so the recording cuts it where its owner went).
#[allow(clippy::too_many_arguments)]
fn start(commands: &mut Commands, log: &mut String, now: f32, file: Handle<AudioSource>, volume: f32, speed: f32, looped: bool, tag: &Tag) {
    let mode = if looped { PlaybackMode::Loop } else { PlaybackMode::Despawn };
    let settings = PlaybackSettings { mode, volume: Volume::Linear(MASTER * volume), speed, ..PlaybackSettings::ONCE };
    if let Some(path) = file.path() {
        let kind = match tag {
            Tag::Loop(m) => format!("loop\t{m}"),
            Tag::Owned(o) => format!("{}\t{}", if looped { "loop" } else { "once" }, o.1),
            Tag::None => "once".to_string(),
        };
        *log += &format!("{now:.4}\t{}\t{:.4}\t{speed:.4}\t{kind}\n", path.path().display(), MASTER * volume);
    }
    let mut sound = commands.spawn((AudioPlayer::new(file), settings));
    match tag {
        Tag::Loop(m) => {
            sound.insert(LoopOf(m.clone()));
        }
        Tag::Owned(o) => {
            sound.insert(o.clone());
        }
        Tag::None => {}
    }
}

fn write_log(log: Option<&SoundLog>, lines: &str) {
    if let Some(log) = log
        && !lines.is_empty()
    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log.0) {
            let _ = f.write_all(lines.as_bytes());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    time: Res<Time>,
    log: Option<Res<SoundLog>>,
    mut cues: MessageReader<SoundCue>,
    mut sounds: ResMut<Sounds>,
    settings: Res<Settings>,
    players: Query<&Player>,
    loops: Query<(Entity, &LoopOf)>,
    mut commands: Commands,
) {
    if !sounds.available() {
        cues.clear();
        return;
    }
    let now = time.elapsed_secs();
    let listener = players.single().ok().map(|p| p.feet);
    let mut lines = String::new();
    let cues: Vec<SoundCue> = cues.read().cloned().collect();
    for (k, cue) in cues.iter().enumerate() {
        // Started and stopped again within one frame (a glide one tick long): the stop can't see
        // a loop that only starts at the end of the frame, so it never starts.
        if cue.looping == Some(true) && cues[k + 1..].iter().any(|c| c.looping == Some(false) && c.name == cue.name) {
            continue;
        }
        let moment = if sounds.moments.contains_key(&cue.name) { cue.name.clone() } else { sounds.events.get(&cue.name).cloned().unwrap_or_default() };
        let Some(chance) = sounds.moments.get(&moment).map(|m| m.chance) else { continue };
        match cue.looping {
            Some(false) => {
                for (e, l) in &loops {
                    if l.0 == moment {
                        commands.entity(e).despawn();
                    }
                }
                lines += &format!("{now:.4}\t{moment}\t0\t1\tstop\n");
                continue;
            }
            Some(true) if loops.iter().any(|(_, l)| l.0 == moment) => continue,
            _ => {}
        }
        if chance < 1.0 && sounds.unit() >= chance {
            continue;
        }
        let gain = match (cue.at, listener) {
            (Some(at), Some(l)) => falloff(at.distance(l) / settings.scale),
            _ => 1.0,
        };
        // Played on an owner, its loop layers loop until the owner goes, after their delay.
        let owned = cue.owner.map(|o| OwnedBy(o, format!("{moment}@{o}")));
        let looping = cue.looping == Some(true) || owned.is_some();
        let tag = |looped: bool| match &owned {
            Some(o) => Tag::Owned(o.clone()),
            None if looped => Tag::Loop(moment.clone()),
            None => Tag::None,
        };
        for i in 0..sounds.moments[&moment].layers.len() {
            let (delay, looped, interval) = {
                let l = &sounds.moments[&moment].layers[i];
                (l.delay, l.looped, l.interval)
            };
            let looped = looping && looped;
            if looped && let Some(every) = interval {
                // Retriggered loop: a fresh file every interval, from the first one on.
                let driver = Retrigger { moment: moment.clone(), layer: i, every, left: delay, tag: tag(false) };
                match &owned {
                    Some(o) => commands.spawn((driver, o.clone())),
                    None => commands.spawn((driver, LoopOf(moment.clone()))),
                };
                continue;
            }
            let Some((file, volume, speed)) = sounds.pick(&moment, i) else { continue };
            let volume = volume * gain;
            // A loop that isn't owned starts at once (its stop cue has to find it).
            if delay > 0.0 && (owned.is_some() || !looped) {
                let mut pending = commands.spawn(Delayed { left: delay, file, volume, speed, looped, tag: tag(looped) });
                if let Some(o) = &owned {
                    pending.insert(o.clone());
                }
            } else {
                start(&mut commands, &mut lines, now, file, volume, speed, looped, &tag(looped));
            }
        }
    }
    write_log(log.as_deref(), &lines);
}

fn delayed(time: Res<Time>, log: Option<Res<SoundLog>>, mut q: Query<(Entity, &mut Delayed)>, mut commands: Commands) {
    let mut lines = String::new();
    for (e, mut d) in &mut q {
        d.left -= time.delta_secs();
        if d.left <= 0.0 {
            start(&mut commands, &mut lines, time.elapsed_secs(), d.file.clone(), d.volume, d.speed, d.looped, &d.tag);
            commands.entity(e).despawn();
        }
    }
    write_log(log.as_deref(), &lines);
}

fn retrigger(time: Res<Time>, log: Option<Res<SoundLog>>, mut sounds: ResMut<Sounds>, mut q: Query<&mut Retrigger>, mut commands: Commands) {
    let mut lines = String::new();
    for mut r in &mut q {
        r.left -= time.delta_secs();
        if r.left <= 0.0 {
            r.left += r.every;
            if let Some((file, volume, speed)) = sounds.pick(&r.moment, r.layer) {
                start(&mut commands, &mut lines, time.elapsed_secs(), file, volume, speed, false, &r.tag);
            }
        }
    }
    write_log(log.as_deref(), &lines);
}

/// Stops what was played on an entity once the entity is gone (the grenade's flight at its
/// blast), sounds still waiting on their delay included.
fn stop_orphans(time: Res<Time>, log: Option<Res<SoundLog>>, entities: &bevy::ecs::entity::Entities, q: Query<(Entity, &OwnedBy)>, mut commands: Commands) {
    let mut gone: Vec<&str> = vec![];
    for (e, o) in &q {
        if !entities.contains(o.0) {
            commands.entity(e).despawn();
            if !gone.contains(&o.1.as_str()) {
                gone.push(&o.1);
            }
        }
    }
    let now = time.elapsed_secs();
    let lines: String = gone.iter().map(|key| format!("{now:.4}\t{key}\t0\t1\tstop\n")).collect();
    write_log(log.as_deref(), &lines);
}

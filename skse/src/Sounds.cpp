// SPDX-License-Identifier: GPL-3.0-or-later
#include "Sounds.h"

#include "Settings.h"

#include <sstream>
#include <unordered_map>
#include <unordered_set>

namespace Sounds
{
	namespace
	{
		// The sandbox's mix (apps/sandbox/src/sound.rs): the game's moments span about 45 dB of
		// levels before its own buses and loudness mixing, which neither has; they play at half
		// their difference from 24 dB, within -18..+3 dB, times each file's own volume, times 0.7
		// overall. Skyrim's sound volume applies on top, and the INI's fSoundVolume.
		constexpr float kMaster = 0.7f;
		constexpr float kLevelReference = 24.0f;
		constexpr float kLevelScale = 0.5f;
		constexpr float kLevelMin = -18.0f;
		constexpr float kLevelMax = 3.0f;
		// A sound away from him is full within 6 m (Ratchet-sized), then fades (sound.rs falloff).
		constexpr float kFullWithin = 6.0f;
		// Stopping a sound fades it this long (ms): cut dead, it clicks.
		constexpr std::uint16_t kStopFade = 30;

		struct File
		{
			std::string path;  // under Data
			float       seconds = 1.0f;
			float       volume = 1.0f;  // its own, times its layer's
			std::string loopPath;       // a looping layer's: the file over and over (baked by the converter)
			float       loopSeconds = 0.0f;
		};

		struct Layer
		{
			float             delay = 0.0f;
			bool              loop = false;
			float             pitchLow = 0.0f;  // cents
			float             pitchHigh = 0.0f;
			float             interval = 0.0f;  // looping: a fresh file this often (s) instead of one looping
			std::vector<File> files;
		};

		struct Moment
		{
			float              gain = 1.0f;
			float              chance = 1.0f;
			std::vector<Layer> layers;
		};

		// One sound of a moment, waiting on its delay or playing; or a layer's driver, playing a
		// fresh file of it every interval while its loop lasts.
		struct Voice
		{
			const Moment*     moment = nullptr;
			std::size_t       layer = 0;
			std::string       loop;            // the loop it came from (it follows the loop's place)
			bool              looping = false; // and ends with the loop (its loop layers do; the rest play out)
			std::string       owner;           // what it belongs to: it follows that, and ends with it
			bool              fades = true;    // a world sound: quieter far from him (a loop's and an owner's aren't)
			RE::NiPoint3      where;
			float             in = 0.0f;       // seconds until it starts (a driver: until its next file)
			float             every = 0.0f;    // > 0: a driver
			std::string       file;
			float             volume = 1.0f;   // before the master volumes and the distance
			float             frequency = 1.0f;
			bool              looped = false;  // its baked loop, again each time it's over
			float             length = 0.0f;   // seconds, at its frequency
			bool              started = false;
			double            endsAt = 0.0;
			RE::BSSoundHandle handle;
		};

		std::unordered_map<std::string, Moment> g_moments;
		std::vector<Voice>                      g_voices;
		std::unordered_set<std::string>         g_loops;  // the loops started and not stopped
		// Per loop: when it started, and the files it has played (logged at its stop).
		std::unordered_map<std::string, std::pair<double, int>> g_loopStats;
		// Per loop: the frame Loop last kept it up in. One its caller has let go of (he was handed
		// back to Skyrim mid-glide, say) stops itself kLoopStale frames on (frames, not seconds: a
		// hitch isn't letting go).
		std::unordered_map<std::string, std::uint64_t> g_loopKept;
		std::uint64_t                                  g_frame = 0;
		constexpr std::uint64_t                        kLoopStale = 10;
		double                                  g_clock = 0.0;
		std::uint32_t                           g_random = 0x2545F491u;
		bool                                    g_warned = false;

		// xorshift32, as the sandbox's.
		std::uint32_t Next()
		{
			g_random ^= g_random << 13;
			g_random ^= g_random >> 17;
			g_random ^= g_random << 5;
			return g_random;
		}

		float Unit() { return static_cast<float>(Next() >> 8) / 16777216.0f; }

		// How loud a sound at a_where is from him (sound.rs falloff: his own are at his feet).
		float Falloff(const RE::NiPoint3& a_where)
		{
			const auto* player = RE::PlayerCharacter::GetSingleton();
			if (!player) {
				return 1.0f;
			}
			const float metres = a_where.GetDistance(player->GetPosition()) / RE::bhkWorld::GetWorldScaleInverse() / Settings::Get().scale;
			return metres <= kFullWithin ? 1.0f : std::pow(kFullWithin / metres, 0.85f);
		}

		RE::BSSoundHandle Start(const Voice& a_voice)
		{
			RE::BSSoundHandle handle;
			auto* audio = RE::BSAudioManager::GetSingleton();
			if (!audio) {
				return handle;
			}
			RE::BSResource::ID id;
			id.GenerateFromPath(a_voice.file.c_str());
			audio->GetSoundHandleByFile(handle, id, 0x1A, 128);
			if (!handle.IsValid()) {
				if (!g_warned) {
					g_warned = true;
					SKSE::log::warn("sounds: Skyrim won't play {} (no more of these warnings)", a_voice.file);
				}
				return handle;
			}
			handle.SetPosition(a_voice.where);
			handle.SetVolume(std::clamp(kMaster * a_voice.volume * (a_voice.fades ? Falloff(a_voice.where) : 1.0f) * Settings::Get().soundVolume, 0.0f, 1.0f));
			if (a_voice.frequency != 1.0f) {
				handle.SetFrequency(a_voice.frequency);
			}
			handle.Play();
			return handle;
		}

		void Begin(Voice& a_voice)
		{
			a_voice.handle = Start(a_voice);
			a_voice.started = true;
			a_voice.endsAt = g_clock + a_voice.length;
			if (!a_voice.loop.empty() && a_voice.handle.IsValid()) {
				if (const auto it = g_loopStats.find(a_voice.loop); it != g_loopStats.end()) {
					++it->second.second;
				}
			}
		}

		// One file of the voice's layer, at a pitch in its range (sound.rs pick); a_looped: its
		// baked loop.
		void Pick(Voice& a_voice, bool a_looped)
		{
			const Layer& layer = a_voice.moment->layers[a_voice.layer];
			const File&  file = layer.files[Next() % layer.files.size()];
			const float  cents = layer.pitchLow + (layer.pitchHigh - layer.pitchLow) * Unit();
			a_voice.frequency = std::pow(2.0f, cents / 1200.0f);
			a_voice.looped = a_looped && !file.loopPath.empty();
			a_voice.file = a_voice.looped ? file.loopPath : file.path;
			a_voice.length = (a_voice.looped ? file.loopSeconds : file.seconds) / a_voice.frequency;
			a_voice.volume = file.volume * a_voice.moment->gain;
		}

		// A moment's cue (sound.rs play): each layer once. A loop's and an owner's loop layers loop
		// (a retriggered one through a driver); only an owner's wait for their delay.
		void Cue(const Moment& a_moment, const RE::NiPoint3& a_where, float a_after, const std::string& a_loop, const std::string& a_owner)
		{
			const bool looping = !a_loop.empty() || !a_owner.empty();
			for (std::size_t i = 0; i < a_moment.layers.size(); ++i) {
				const Layer& layer = a_moment.layers[i];
				if (layer.files.empty()) {
					continue;
				}
				const bool looped = looping && layer.loop;
				Voice      v;
				v.moment = &a_moment;
				v.layer = i;
				v.loop = a_loop;
				v.looping = looped && !a_loop.empty();
				v.owner = a_owner;
				// (sound.rs: only a cue at a place fades; a loop on him and an owner's play as they are.)
				v.fades = a_loop.empty() && a_owner.empty();
				v.where = a_where;
				if (looped && layer.interval > 0.0f) {
					v.every = layer.interval;
					v.in = layer.delay + a_after;
				} else {
					Pick(v, looped);
					v.in = (looped && a_owner.empty() ? 0.0f : layer.delay) + a_after;
				}
				g_voices.push_back(std::move(v));
			}
		}

		bool Roll(const Moment& a_moment) { return a_moment.chance >= 1.0f || Unit() < a_moment.chance; }

		void Silence(Voice& a_voice)
		{
			if (a_voice.started) {
				a_voice.handle.FadeOutAndRelease(kStopFade);
			}
		}
	}

	bool Load(const std::filesystem::path& a_pluginDir)
	{
		if (!Settings::Get().sounds) {
			return false;
		}
		const auto path = a_pluginDir / L"RatchetMovement" / L"sounds.txt";
		std::ifstream in(path);
		if (!in) {
			SKSE::log::info("sounds: no {} (skse/tools/ratchet_sounds_to_skyrim.py makes it): silent", path.string());
			return false;
		}
		std::string line;
		Moment*     current = nullptr;
		std::size_t files = 0;
		std::size_t loops = 0;
		while (std::getline(in, line)) {
			std::istringstream words(line);
			std::string        kind;
			words >> kind;
			if (kind == "moment") {
				std::string name;
				float       level = kLevelReference;
				Moment      m;
				words >> name >> level >> m.chance;
				m.gain = std::pow(10.0f, std::clamp((level - kLevelReference) * kLevelScale, kLevelMin, kLevelMax) / 20.0f);
				current = &(g_moments[name] = std::move(m));
			} else if (kind == "layer" && current) {
				Layer l;
				float volume = 1.0f;
				int   loop = 0;
				words >> volume >> l.delay >> loop >> l.pitchLow >> l.pitchHigh;
				l.loop = loop != 0;
				std::vector<std::string> rest;
				for (std::string w; words >> w;) {
					rest.push_back(std::move(w));
				}
				// (An older sounds.txt has no interval: its files follow the pitch.)
				std::size_t first = 0;
				if (!rest.empty() && rest[0].find('|') == std::string::npos) {
					l.interval = std::strtof(rest[0].c_str(), nullptr);
					first = 1;
				}
				for (std::size_t i = first; i < rest.size(); ++i) {
					std::vector<std::string> parts;
					for (std::size_t from = 0;;) {
						const auto bar = rest[i].find('|', from);
						parts.push_back(rest[i].substr(from, bar == std::string::npos ? std::string::npos : bar - from));
						if (bar == std::string::npos) {
							break;
						}
						from = bar + 1;
					}
					File file;
					file.path = parts[0];
					file.seconds = parts.size() > 1 ? std::strtof(parts[1].c_str(), nullptr) : 1.0f;
					file.volume = volume * (parts.size() > 2 ? std::strtof(parts[2].c_str(), nullptr) : 1.0f);
					if (parts.size() > 4) {
						file.loopPath = parts[3];
						file.loopSeconds = std::strtof(parts[4].c_str(), nullptr);
						++loops;
					}
					l.files.push_back(std::move(file));
				}
				files += l.files.size();
				current->layers.push_back(std::move(l));
			}
		}
		SKSE::log::info("sounds: {} moments, {} files ({} looping)", g_moments.size(), files, loops);
		return !g_moments.empty();
	}

	void Play(std::string_view a_moment, const RE::NiPoint3& a_where, float a_after, std::string_view a_owner)
	{
		const auto it = g_moments.find(std::string(a_moment));
		if (it == g_moments.end() || !Roll(it->second)) {
			return;
		}
		Cue(it->second, a_where, a_after, {}, std::string(a_owner));
	}

	void Follow(std::string_view a_owner, const RE::NiPoint3& a_where)
	{
		for (auto& v : g_voices) {
			if (v.owner == a_owner) {
				v.where = a_where;
				if (v.started) {
					v.handle.SetPosition(a_where);
				}
			}
		}
	}

	void StopOwned(std::string_view a_owner)
	{
		for (auto& v : g_voices) {
			if (v.owner == a_owner) {
				Silence(v);
			}
		}
		std::erase_if(g_voices, [&](const Voice& v) { return v.owner == a_owner; });
	}

	void Loop(std::string_view a_moment, const RE::NiPoint3& a_where)
	{
		g_loopKept[std::string(a_moment)] = g_frame;
		// Started once (a second start does nothing), then it follows a_where.
		if (const std::string name(a_moment); g_loops.insert(name).second) {
			const auto it = g_moments.find(name);
			if (it == g_moments.end() || !Roll(it->second)) {
				return;
			}
			g_loopStats[name] = { g_clock, 0 };
			Cue(it->second, a_where, 0.0f, name, {});
		}
		for (auto& v : g_voices) {
			if (v.loop == a_moment) {
				v.where = a_where;
				if (v.started) {
					v.handle.SetPosition(a_where);
				}
			}
		}
	}

	void Stop(std::string_view a_moment)
	{
		g_loops.erase(std::string(a_moment));
		if (const auto it = g_loopStats.find(std::string(a_moment)); it != g_loopStats.end()) {
			SKSE::log::info("sounds: {} stopped after {:.2f} s, {} files played", a_moment, g_clock - it->second.first, it->second.second);
			g_loopStats.erase(it);
		}
		for (auto& v : g_voices) {
			if (v.looping && v.loop == a_moment) {
				Silence(v);
			}
		}
		std::erase_if(g_voices, [&](const Voice& v) { return v.looping && v.loop == a_moment; });
	}

	void Reset()
	{
		for (auto& v : g_voices) {
			Silence(v);
		}
		g_voices.clear();
		g_loops.clear();
		g_loopKept.clear();
		g_loopStats.clear();
	}

	namespace
	{
		RE::BSSoundHandle g_test;
	}

	void PlayTest(const std::string& a_file, const RE::NiPoint3& a_where, float a_volume)
	{
		StopTest();
		auto* audio = RE::BSAudioManager::GetSingleton();
		if (!audio) {
			return;
		}
		RE::BSResource::ID id;
		id.GenerateFromPath(a_file.c_str());
		audio->GetSoundHandleByFile(g_test, id, 0x1A, 128);
		if (!g_test.IsValid()) {
			SKSE::log::warn("sounds: the lab's {} won't play", a_file);
			return;
		}
		g_test.SetPosition(a_where);
		g_test.SetVolume(a_volume);
		const bool played = g_test.Play();
		SKSE::log::info("sounds: the lab's sound {:08X}: {}, {}", g_test.soundID, played ? "played" : "not played", g_test.IsPlaying() ? "playing" : "not playing yet");
	}

	void StopTest()
	{
		if (g_test.IsValid()) {
			g_test.Stop();
		}
		g_test = RE::BSSoundHandle{};
	}

	void Update(float a_dt)
	{
		g_clock += a_dt;
		++g_frame;
		// Loops nobody keeps up any more stop (Update runs only while the game does: a pause isn't
		// letting go either).
		std::vector<std::string> stale;
		for (const auto& name : g_loops) {
			if (const auto it = g_loopKept.find(name); it == g_loopKept.end() || g_frame - it->second > kLoopStale) {
				stale.push_back(name);
			}
		}
		for (const auto& name : stale) {
			SKSE::log::info("sounds: {} wasn't kept up: stopped", name);
			Stop(name);
		}
		std::vector<Voice> fresh;  // the drivers' new files
		for (auto& v : g_voices) {
			if (v.every > 0.0f) {
				// A retriggered loop: a fresh file every interval, from the first on (sound.rs
				// retrigger; it plays out, unless its owner goes).
				v.in -= a_dt;
				if (v.in <= 0.0f) {
					v.in += v.every;
					Voice one = v;
					one.every = 0.0f;
					one.looping = false;
					Pick(one, false);
					Begin(one);
					fresh.push_back(std::move(one));
				}
			} else if (!v.started) {
				v.in -= a_dt;
				if (v.in <= 0.0f) {
					Begin(v);
				}
			} else if (v.looped && g_clock >= v.endsAt) {
				// The baked loop is over: again (its seams inside are the file's own).
				v.handle = Start(v);
				v.endsAt += v.length;
			}
		}
		// Played out: gone (their handles end by themselves).
		std::erase_if(g_voices, [](const Voice& v) { return v.started && !v.looped && v.every == 0.0f && g_clock >= v.endsAt + 0.1; });
		for (auto& v : fresh) {
			g_voices.push_back(std::move(v));
		}
	}
}

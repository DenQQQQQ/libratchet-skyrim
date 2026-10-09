// SPDX-License-Identifier: GPL-3.0-or-later
#include "Sounds.h"

#include "Settings.h"

#include <sstream>
#include <unordered_map>

namespace Sounds
{
	namespace
	{
		// The game mixes its moments by level (dB): a moment this loud plays at full volume, and
		// quieter ones (his footsteps' foley at -9 dB) below it. Louder ones play at full volume
		// too, their layers in their own mix (the Shatterbomb's blast, at 35 dB, had all eight of
		// its layers pushed to full and crackled). Ours: Skyrim has no mixer to match, and its own
		// sound volume applies on top.
		constexpr float kFullLevelDb = 15.0f;

		struct File
		{
			std::string path;  // under Data
			float       seconds = 0.0f;
		};

		struct Layer
		{
			float             volume = 1.0f;
			float             delay = 0.0f;
			bool              loop = false;
			float             pitchLow = 0.0f;  // cents
			float             pitchHigh = 0.0f;
			std::vector<File> files;
		};

		struct Moment
		{
			float              levelDb = 0.0f;
			float              chance = 1.0f;
			std::vector<Layer> layers;
		};

		struct Pending
		{
			float        in = 0.0f;  // seconds until it starts
			std::string  file;
			float        volume = 1.0f;
			float        frequency = 1.0f;
			RE::NiPoint3 where;
			std::string  loop;   // the looping moment it belongs to, if any
			std::string  owner;  // what it belongs to (a Shatterbomb in the air), if anything
		};

		// A looping moment: a round of its layers, and the next round when the longest is over
		// (Skyrim's handles don't say reliably whether a sound is still playing).
		struct Looping
		{
			std::vector<RE::BSSoundHandle> handles;
			double                          nextRound = 0.0;
		};

		std::unordered_map<std::string, Moment>  g_moments;
		std::vector<Pending>                     g_pending;
		std::unordered_map<std::string, Looping> g_loops;
		std::unordered_map<std::string, std::vector<RE::BSSoundHandle>> g_owned;
		double                                   g_clock = 0.0;
		std::uint32_t                            g_random = 0x2545F491u;
		bool                                     g_warned = false;

		float Random01()
		{
			g_random ^= g_random << 13;
			g_random ^= g_random >> 17;
			g_random ^= g_random << 5;
			return static_cast<float>(g_random >> 8) / 16777216.0f;
		}

		RE::BSSoundHandle Start(const Pending& a_p)
		{
			RE::BSSoundHandle handle;
			auto* audio = RE::BSAudioManager::GetSingleton();
			if (!audio) {
				return handle;
			}
			RE::BSResource::ID id;
			id.GenerateFromPath(a_p.file.c_str());
			audio->GetSoundHandleByFile(handle, id, 0x1A, 128);
			if (!handle.IsValid()) {
				if (!g_warned) {
					g_warned = true;
					SKSE::log::warn("sounds: Skyrim won't play {} (no more of these warnings)", a_p.file);
				}
				return handle;
			}
			handle.SetPosition(a_p.where);
			handle.SetVolume(a_p.volume);
			if (a_p.frequency != 1.0f) {
				handle.SetFrequency(a_p.frequency);
			}
			handle.Play();
			return handle;
		}

		// Each layer of a moment, one of its files, now or after its delay. Returns how long
		// the round lasts (its longest layer, from now).
		float Queue(const Moment& a_moment, const RE::NiPoint3& a_where, const std::string& a_loop, float a_after, const std::string& a_owner = {})
		{
			const float master = Settings::Get().soundVolume;
			const float level = std::min(1.0f, std::pow(10.0f, (a_moment.levelDb - kFullLevelDb) / 20.0f));
			float       longest = 0.0f;
			for (const Layer& layer : a_moment.layers) {
				if (layer.files.empty() || layer.volume <= 0.0f) {
					continue;
				}
				const File& file = layer.files[std::min(layer.files.size() - 1, static_cast<std::size_t>(Random01() * layer.files.size()))];
				const float cents = layer.pitchLow + (layer.pitchHigh - layer.pitchLow) * Random01();
				Pending p;
				p.in = layer.delay + a_after;
				p.file = file.path;
				p.volume = std::clamp(level * layer.volume * master, 0.0f, 1.0f);
				p.frequency = std::pow(2.0f, cents / 1200.0f);
				p.where = a_where;
				p.loop = a_loop;
				p.owner = a_owner;
				// A higher pitch plays the file faster.
				longest = std::max(longest, p.in + file.seconds / p.frequency);
				g_pending.push_back(std::move(p));
			}
			return longest;
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
		while (std::getline(in, line)) {
			std::istringstream words(line);
			std::string        kind;
			words >> kind;
			if (kind == "moment") {
				std::string name;
				Moment      m;
				words >> name >> m.levelDb >> m.chance;
				current = &(g_moments[name] = std::move(m));
			} else if (kind == "layer" && current) {
				Layer l;
				int   loop = 0;
				words >> l.volume >> l.delay >> loop >> l.pitchLow >> l.pitchHigh;
				l.loop = loop != 0;
				for (std::string f; words >> f;) {
					File        file;
					const auto  bar = f.find('|');
					file.path = f.substr(0, bar);
					file.seconds = bar == std::string::npos ? 1.0f : std::strtof(f.c_str() + bar + 1, nullptr);
					l.files.push_back(std::move(file));
				}
				files += l.files.size();
				current->layers.push_back(std::move(l));
			}
		}
		SKSE::log::info("sounds: {} moments, {} files", g_moments.size(), files);
		return !g_moments.empty();
	}

	void Play(std::string_view a_moment, const RE::NiPoint3& a_where, float a_after, std::string_view a_owner)
	{
		const auto it = g_moments.find(std::string(a_moment));
		if (it == g_moments.end() || Random01() >= it->second.chance) {
			return;
		}
		Queue(it->second, a_where, {}, a_after, std::string(a_owner));
	}

	void Follow(std::string_view a_owner, const RE::NiPoint3& a_where)
	{
		const std::string owner(a_owner);
		if (const auto it = g_owned.find(owner); it != g_owned.end()) {
			for (auto& handle : it->second) {
				handle.SetPosition(a_where);
			}
		}
		for (auto& p : g_pending) {
			if (p.owner == owner) {
				p.where = a_where;
			}
		}
	}

	void StopOwned(std::string_view a_owner)
	{
		const std::string owner(a_owner);
		if (const auto it = g_owned.find(owner); it != g_owned.end()) {
			for (auto& handle : it->second) {
				handle.FadeOutAndRelease(50);
			}
			g_owned.erase(it);
		}
		std::erase_if(g_pending, [&](const Pending& p) { return p.owner == owner; });
	}

	void Loop(std::string_view a_moment, const RE::NiPoint3& a_where)
	{
		const std::string name(a_moment);
		const auto        it = g_moments.find(name);
		if (it == g_moments.end()) {
			return;
		}
		auto& loop = g_loops[name];
		for (auto& handle : loop.handles) {
			handle.SetPosition(a_where);
		}
		if (g_clock >= loop.nextRound) {
			loop.handles.clear();
			loop.nextRound = g_clock + std::max(0.05f, Queue(it->second, a_where, name, 0.0f));
		}
	}

	void Stop(std::string_view a_moment)
	{
		const std::string name(a_moment);
		if (const auto it = g_loops.find(name); it != g_loops.end()) {
			for (auto& handle : it->second.handles) {
				handle.FadeOutAndRelease(100);
			}
			g_loops.erase(it);
		}
		std::erase_if(g_pending, [&](const Pending& p) { return p.loop == name; });
	}

	void Update(float a_dt)
	{
		g_clock += a_dt;
		for (std::size_t i = 0; i < g_pending.size();) {
			auto& p = g_pending[i];
			p.in -= a_dt;
			if (p.in > 0.0f) {
				++i;
				continue;
			}
			const Pending started = std::move(p);
			g_pending.erase(g_pending.begin() + static_cast<std::ptrdiff_t>(i));
			auto handle = Start(started);
			if (!started.owner.empty() && handle.IsValid()) {
				g_owned[started.owner].push_back(handle);
			}
			if (!started.loop.empty() && handle.IsValid()) {
				if (const auto it = g_loops.find(started.loop); it != g_loops.end()) {
					it->second.handles.push_back(handle);
				} else {
					handle.Stop();  // stopped before it began
				}
			}
		}
	}
}

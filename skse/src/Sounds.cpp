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
		// quieter ones (his footsteps' foley at -9 dB) below it. Ours: Skyrim has no mixer to
		// match, and its own sound volume applies on top.
		constexpr float kFullLevelDb = 15.0f;

		struct Layer
		{
			float                    volume = 1.0f;
			float                    delay = 0.0f;
			bool                     loop = false;
			float                    pitchLow = 0.0f;  // cents
			float                    pitchHigh = 0.0f;
			std::vector<std::string> files;  // paths under Data
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
			std::string  loop;  // the looping moment it belongs to, if any
		};

		struct Looping
		{
			RE::NiPoint3                    where;
			std::vector<RE::BSSoundHandle> handles;
			bool                            started = false;
		};

		std::unordered_map<std::string, Moment>  g_moments;
		std::vector<Pending>                     g_pending;
		std::unordered_map<std::string, Looping> g_loops;
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

		// Each layer of a moment, one of its files, now or after its delay.
		void Queue(const Moment& a_moment, const RE::NiPoint3& a_where, const std::string& a_loop)
		{
			const float master = Settings::Get().soundVolume;
			for (const Layer& layer : a_moment.layers) {
				if (layer.files.empty() || layer.volume <= 0.0f) {
					continue;
				}
				Pending p;
				p.in = layer.delay;
				p.file = layer.files[std::min(layer.files.size() - 1, static_cast<std::size_t>(Random01() * layer.files.size()))];
				p.volume = std::clamp(std::pow(10.0f, (a_moment.levelDb - kFullLevelDb) / 20.0f) * layer.volume * master, 0.0f, 1.0f);
				const float cents = layer.pitchLow + (layer.pitchHigh - layer.pitchLow) * Random01();
				p.frequency = std::pow(2.0f, cents / 1200.0f);
				p.where = a_where;
				p.loop = a_loop;
				g_pending.push_back(std::move(p));
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
		while (std::getline(in, line)) {
			std::istringstream words(line);
			std::string kind;
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
					l.files.push_back(std::move(f));
				}
				files += l.files.size();
				current->layers.push_back(std::move(l));
			}
		}
		SKSE::log::info("sounds: {} moments, {} files", g_moments.size(), files);
		return !g_moments.empty();
	}

	void Play(std::string_view a_moment, const RE::NiPoint3& a_where, float a_after)
	{
		const auto it = g_moments.find(std::string(a_moment));
		if (it == g_moments.end() || Random01() >= it->second.chance) {
			return;
		}
		const std::size_t first = g_pending.size();
		Queue(it->second, a_where, {});
		for (std::size_t i = first; i < g_pending.size(); ++i) {
			g_pending[i].in += a_after;
		}
	}

	void Loop(std::string_view a_moment, const RE::NiPoint3& a_where)
	{
		const std::string name(a_moment);
		if (!g_moments.contains(name)) {
			return;
		}
		auto& loop = g_loops[name];
		loop.where = a_where;
		for (auto& handle : loop.handles) {
			handle.SetPosition(a_where);
		}
		std::erase_if(loop.handles, [](const RE::BSSoundHandle& h) { return !h.IsPlaying(); });
		const bool waiting = std::ranges::any_of(g_pending, [&](const Pending& p) { return p.loop == name; });
		if (loop.handles.empty() && !waiting) {
			Queue(g_moments[name], a_where, name);
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

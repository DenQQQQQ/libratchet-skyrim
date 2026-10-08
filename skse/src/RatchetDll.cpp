// SPDX-License-Identifier: GPL-3.0-or-later
#include "RatchetDll.h"

#include "Settings.h"

namespace Ratchet
{
	namespace
	{
		bool                     g_loaded = false;
		std::filesystem::path    g_pluginDir;
		std::optional<JumpNumbers> g_expected;

		struct Apex
		{
			float height = 0.0f;
			float time = 0.0f;
			float airTime = 0.0f;
		};

		// The sandbox's flat-floor jump (crates/libratchet/tests/movement.rs `jump`): the floor at
		// y = 0, the button held for `a_hold` seconds.
		Apex Jump(RatchetSim* a_sim, float a_dt, float a_hold)
		{
			ratchet_reset(a_sim);
			RatchetVec3 feet{ 0.0f, 0.0f, 0.0f };
			RatchetContact contact{};
			contact.grounded = 1;
			contact.has_ground_distance = 1;
			Apex apex;
			bool airborne = false;
			float t = 0.0f;
			for (int i = 0; i < static_cast<int>(4.0f / a_dt); ++i) {
				RatchetInput in{};
				in.jump = t < a_hold ? 1 : 0;
				const RatchetStepResult r = ratchet_step(a_sim, a_dt, in, contact, feet);
				feet.x += r.velocity.x * a_dt;
				feet.y = std::max(0.0f, feet.y + r.velocity.y * a_dt);
				feet.z += r.velocity.z * a_dt;
				t += a_dt;
				if (feet.y > apex.height) {
					apex.height = feet.y;
					apex.time = t;
				}
				airborne = airborne || feet.y > 0.0f;
				contact.grounded = feet.y <= 0.0f ? 1 : 0;
				contact.ground_distance = feet.y;
				if (airborne && contact.grounded) {
					apex.airTime = t;
					break;
				}
			}
			return apex;
		}
	}

	bool Load(const std::filesystem::path& a_pluginDir)
	{
		g_pluginDir = a_pluginDir;
		// In a subfolder: SKSE tries every DLL in Plugins as a plugin.
		const auto dll = a_pluginDir / L"RatchetMovement" / L"ratchet.dll";
		// The full path, so Windows doesn't search; the delay-load helper then finds this module
		// by name.
		HMODULE h = LoadLibraryExW(dll.c_str(), nullptr, LOAD_WITH_ALTERED_SEARCH_PATH);
		if (!h) {
			SKSE::log::critical("couldn't load {} (error {})", dll.string(), GetLastError());
			return false;
		}
		const std::uint32_t version = ratchet_api_version();
		if (version != RATCHET_API_VERSION) {
			SKSE::log::critical("ratchet.dll has C API version {}, the plugin was built for {}: rebuild both", version, RATCHET_API_VERSION);
			return false;
		}
		SKSE::log::info("loaded {} (C API version {})", dll.string(), version);
		g_loaded = true;
		return true;
	}

	bool Loaded() { return g_loaded; }

	RatchetSim* NewSim()
	{
		const auto& s = Settings::Get();
		RatchetSim* sim = ratchet_new(s.scale);
		if (!s.configsDir.empty()) {
			std::filesystem::path dir = std::filesystem::path(reinterpret_cast<const char8_t*>(s.configsDir.c_str()));
			if (dir.is_relative()) {
				dir = g_pluginDir / dir;
			}
			std::error_code ec;
			if (std::filesystem::is_directory(dir, ec)) {
				const auto utf8 = dir.u8string();
				const std::int32_t rc = ratchet_load_configs(sim, reinterpret_cast<const char*>(utf8.c_str()), s.scale);
				if (rc == 0) {
					SKSE::log::info("tuning: Rift Apart configs from {}", reinterpret_cast<const char*>(utf8.c_str()));
				} else {
					SKSE::log::warn("tuning: couldn't read the configs in {} (error {}); built-in tuning", reinterpret_cast<const char*>(utf8.c_str()), rc);
				}
			} else {
				SKSE::log::warn("tuning: no configs folder at {}; built-in tuning", dir.string());
			}
		}
		return sim;
	}

	JumpNumbers MeasureFlatJump(float a_dt, float a_hold)
	{
		// Its own sim (made once: loading the configs reads files), so the player's isn't disturbed.
		static RatchetSim* sim = NewSim();
		const Apex tap = Jump(sim, a_dt, a_dt * 0.5f);
		const Apex full = Jump(sim, a_dt, a_hold);
		return { tap.height, tap.time, full.height, full.time, full.airTime };
	}

	Replayed ReplayFlat(const std::vector<Tick>& a_ticks)
	{
		static RatchetSim* sim = NewSim();
		ratchet_reset(sim);
		RatchetVec3 feet{ 0.0f, 0.0f, 0.0f };
		RatchetContact contact{};
		contact.grounded = 1;
		contact.has_ground_distance = 1;
		Replayed out;
		bool airborne = false;
		float t = 0.0f;
		const float lastDt = a_ticks.empty() ? 1.0f / 60.0f : a_ticks.back().dt;
		for (std::size_t i = 0; i < a_ticks.size() + 2000; ++i) {
			const bool recorded = i < a_ticks.size();
			const float dt = recorded ? a_ticks[i].dt : lastDt;
			RatchetInput in{};
			in.jump = recorded && a_ticks[i].jump ? 1 : 0;
			const RatchetStepResult r = ratchet_step(sim, dt, in, contact, feet);
			// The game's frame moves him by this frame's speed (the controller integrates it).
			feet.y = std::max(0.0f, feet.y + r.velocity.y * dt);
			t += dt;
			if (feet.y > out.apex) {
				out.apex = feet.y;
				out.apexTime = t;
			}
			airborne = airborne || feet.y > 0.0f;
			contact.grounded = feet.y <= 0.0f ? 1 : 0;
			contact.ground_distance = feet.y;
			if (airborne && contact.grounded) {
				out.airTime = t;
				break;
			}
		}
		return out;
	}

	const JumpNumbers& Expected()
	{
		if (!g_expected) {
			g_expected = MeasureFlatJump(1.0f / 240.0f);
		}
		return *g_expected;
	}
}

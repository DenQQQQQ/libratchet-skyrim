// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// RatchetMovement.ini, next to the DLL in Data/SKSE/Plugins.
struct Settings
{
	bool enabled = true;
	// 1 = Ratchet-sized; 1.55 = Dragonborn-sized (the plan's default).
	float scale = 1.55f;
	// Rift Apart configs extracted by tools/extract_riftapart_configs.py; relative paths are from
	// the plugin's folder. Empty or missing: libratchet's built-in tuning.
	std::string configsDir = "RatchetMovement";
	// Skyrim's own jump animations (start, fall, land) on the sim's jumps.
	bool animations = true;
	// Ratchet's own model and clips instead of the Dragonborn (needs the converted files).
	bool ratchetModel = true;
	// The Skyrim control (user event) that dashes, on the ground and in the air. Sprint's own
	// sprint is off on foot then (Rift Apart has none). Empty: no dash.
	std::string dashControl = "Sprint";
	// One log line per jump (takeoff, apex, landing) and its expected numbers.
	bool logJumps = true;
	// Per-frame trace while a jump is active (big logs).
	bool traceJumps = false;

	static Settings& Get()
	{
		static Settings s;
		return s;
	}

	void Load(const std::filesystem::path& a_ini);
};

// The folder RatchetMovement.dll was loaded from (Data/SKSE/Plugins, as MO2 shows it).
std::filesystem::path PluginDirectory();

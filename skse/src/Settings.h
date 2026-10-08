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
	// One log line per jump (takeoff, apex, landing) and its expected numbers.
	bool logJumps = true;
	// Per-frame trace while a jump is active (big logs).
	bool traceJumps = false;
	// A test run without anyone at the keyboard: load the most recent save from the main menu,
	// make a few jumps (a tap, a short and a full hold) where the player stands, log them, quit.
	bool autoTest = false;
	bool autoTestQuit = true;

	static Settings& Get()
	{
		static Settings s;
		return s;
	}

	void Load(const std::filesystem::path& a_ini);
};

// The folder RatchetMovement.dll was loaded from (Data/SKSE/Plugins, as MO2 shows it).
std::filesystem::path PluginDirectory();

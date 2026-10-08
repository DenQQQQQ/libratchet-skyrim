// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// RatchetMovement.ini, next to the DLL in Data/SKSE/Plugins.
struct Settings
{
	bool enabled = true;
	// 1 = Ratchet-sized (the owner's choice: lore size is more fun); 1.55 = Dragonborn-sized (the
	// plan's first default).
	float scale = 1.0f;
	// Rift Apart configs extracted by tools/extract_riftapart_configs.py; relative paths are from
	// the plugin's folder. Empty or missing: libratchet's built-in tuning.
	std::string configsDir = "RatchetMovement";
	// Skyrim's own jump animations (start, fall, land) on the sim's jumps.
	bool animations = true;
	// Ratchet's own model and clips instead of the Dragonborn (needs the converted files).
	bool ratchetModel = true;
	// Clank on his back, his heli-pack out while he glides (needs the converted files too).
	bool clank = true;
	// The Skyrim control (user event) that dashes, on the ground and in the air. Sprint's own
	// sprint is off on foot then (Rift Apart has none). Empty: no dash.
	std::string dashControl = "Sprint";
	// The sim walks and runs him too (Rift Apart's speeds, acceleration and turning), not only
	// his jumps and dashes. Off: Skyrim's own locomotion moves him on the ground.
	bool simGround = true;
	// Ratchet's combat: the OmniWrench and his gun on these Skyrim controls (Rift Apart's
	// controller layout: R2 fires, L2 aims, Square swings, R1 throws). Skyrim's own attacks,
	// blocks, weapon drawing and shouts are off on foot then.
	bool        combat = true;
	std::string fireControl = "Right Attack/Block";
	std::string aimControl = "Left Attack/Block";
	std::string meleeControl = "Ready Weapon";
	std::string throwControl = "Shout";
	// Rift Apart's damage numbers times this, on Skyrim's health.
	float damageMult = 1.0f;
	// Rift Apart's sounds on his moves (skse/tools/ratchet_sounds_to_skyrim.py), and how loud.
	bool  sounds = true;
	float soundVolume = 1.0f;
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

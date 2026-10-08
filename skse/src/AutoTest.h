// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

#include "RatchetDll.h"

// Tests in the real game with nobody at the keyboard (skse/test/ingame.ps1 runs them).
//
// A test run starts when SKSE/Plugins/RatchetMovement/autotest.ini exists at game start (the
// plugin deletes it, so the next normal start is a normal game). From the main menu it loads the
// most recent save and goes somewhere to test (by default `coc Whiterun`; no save is written),
// makes its jumps, logs a PASS or FAIL line for each, and quits.
namespace AutoTest
{
	// At plugin load: reads the trigger file (and deletes it).
	void Load(const std::filesystem::path& a_pluginDir);
	bool Active();
	// A save has loaded (the run goes on to its test place).
	void OnGameLoaded();
	// After the game's data loaded: autosaves off (in memory only), and the main menu starts the test.
	void Install();

	// Each frame the jump code runs: the buttons the test holds now. a_busy: the sim is moving
	// him (a flight or a dash).
	struct Buttons
	{
		bool jump = false;
		bool dash = false;
	};
	Buttons Press(float a_dt, bool a_grounded, bool a_busy);

	struct Jump
	{
		float apex;        // metres above the takeoff
		float apexTime;    // seconds after the takeoff
		float hold;        // seconds the first press was held
		float landDz;      // landing height - takeoff height, metres
		float airTime;
		float distance;    // metres across the ground, takeoff to landing
		// The frames' lengths when the apex and the landing were seen: each can only be seen
		// at a frame's end, up to a frame late.
		float apexFrame = 0.0f;
		float landFrame = 0.0f;
		// The sandbox's jump for the same frames on a flat floor (not for a flight that started
		// out of Skyrim's fall or touched a wall or ledge: replayed false).
		bool  replayed = true;
		float sandboxApex = 0.0f;
		float sandboxApexTime = 0.0f;
		float sandboxAirTime = 0.0f;
		float sandboxDistance = 0.0f;
	};
	struct Dash
	{
		float distance;  // metres across the ground, start to end
		float time;      // seconds
		float climb;     // end height - start height, metres
		Ratchet::DashReplayed sandbox;  // the sandbox's dash for the same frames on a flat floor
	};
	// The sim took over: a jump or a ground dash started.
	void OnTakeoff();
	// A moment in the move ("apex", "double_jump", "glide", "dash", "air_dash"): a screenshot of
	// his pose.
	void OnMove(std::string_view a_what);
	void OnLanded(const Jump& a_jump);
	// A ground dash is over.
	void OnDashed(const Dash& a_dash);
	// The jump code gave the player back to Skyrim in the air (he died, was ragdolled...).
	void OnHandedBack(const char* a_why);
}

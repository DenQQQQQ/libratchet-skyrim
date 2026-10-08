// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

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

	// Each frame the jump code runs: the button the test holds now.
	bool JumpButton(float a_dt, bool a_grounded, bool a_inJump);

	struct Jump
	{
		float apex;        // metres above the takeoff
		float apexTime;    // seconds after the takeoff
		float hold;        // seconds the first press was held
		float landDz;      // landing height - takeoff height, metres
		float airTime;
		float distance;    // metres across the ground, takeoff to landing
		// The sandbox's jump for the same frames on a flat floor.
		float sandboxApex;
		float sandboxApexTime;
		float sandboxAirTime;
		float sandboxDistance;
	};
	void OnTakeoff();
	// A moment in the flight ("apex", "double_jump", "glide"): a screenshot of his pose.
	void OnMove(std::string_view a_what);
	void OnLanded(const Jump& a_jump);
	// The jump code gave the player back to Skyrim in the air (he died, was ragdolled...).
	void OnHandedBack(const char* a_why);
}

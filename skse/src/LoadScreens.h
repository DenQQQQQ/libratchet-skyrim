// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart on Skyrim's loading screens: each of Skyrim's loading screens shows one of
// RatchetMovement.esl's models (Ratchet and Clank posed, his weapons, a gold bolt) with one of our
// tips on his moves, in memory (nothing of the game's is changed on disk).
namespace LoadScreens
{
	// After the game's data has loaded.
	void Install();
	// For the tests: what a loading screen shows (its model's path, and its text).
	std::pair<std::string, std::string> Describe(const RE::TESLoadScreen* a_screen);
	// For the tests' pictures: every loading screen shows this model (by its place in the list,
	// wrapping round), so each of them can be looked at in turn. Returns its model's path.
	std::string ShowOnly(int a_look);
}

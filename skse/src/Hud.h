// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart's own HUD pieces, as text fields of their own on Skyrim's HUD (its Scaleform movie,
// set on the UI thread).
namespace Hud
{
	// The bolt counter (the sandbox's bolts.rs): at the top right, "BOLTS  1,234" with the "+N"
	// waiting under it, shown while a_visible.
	void BoltCounter(bool a_visible, std::uint32_t a_shown, std::uint32_t a_gain);
	// For the tests: what the counter's fields say now ("" when there are none, or they're hidden).
	std::pair<std::string, std::string> BoltCounterText();
}

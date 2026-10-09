// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart on Skyrim's main menu: its menu music (the intro, then its loop) instead of Skyrim's
// theme while the menu is up (Skyrim's music category is muted meanwhile, and given back as the
// menu goes). The logo is a file of the mod's (meshes\interface\logo\logo.nif,
// skse\tools\ratchet_menu_to_skyrim.py).
namespace MainMenu
{
	// After the game's data has loaded.
	void Install();
	// For the tests: whether the title music is playing.
	bool MusicPlaying();
}

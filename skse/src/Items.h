// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

#include "Combat.h"

// Ratchet's guns as Skyrim weapons (RatchetMovement.esl, written by skse/tools/ratchet_plugin.py):
// the Burst Pistol and the Shatterbomb are items in his inventory, given once a game, and the one
// he has equipped (either hand) is the gun he uses. With neither, he has his OmniWrench alone.
// Without the plugin file loaded, Combat picks the guns on its own keys as before.
namespace Items
{
	// At the plugin's load: whether this game has had them is kept in SKSE's co-save.
	void Register();
	// After the game's data has loaded: finds them. False: the plugin file isn't loaded.
	bool Install();
	bool Available();
	// Each frame he's on foot: both guns into his inventory the first time in a game (a new game,
	// or a save from before the plugin), with Skyrim's "added" notes.
	void GiveOnce();
	// Which of them he has equipped (the right hand first), if any.
	std::optional<Combat::Gun> Equipped();
	// Tests: equip that one (put in his inventory if he hasn't it), or with kNone take off whichever
	// is equipped. Skyrim equips it a frame or so later.
	void Equip(Combat::Gun a_gun);
	// Tests: how many of each he carries, and whether this game has had them.
	int  Count(Combat::Gun a_gun);
	bool Given();
}

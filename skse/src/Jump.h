// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// libratchet on the player: Skyrim's jump (and sprint) switched off, the sim's jumps, dashes,
// walls, ledges and wrench moving him.
namespace Jump
{
	// After the game's data has loaded: hooks Skyrim's jump input and the player's update.
	void Install();
	// A save loads or a new game starts: forget any move in progress.
	void Reset();
	// The player is on foot and in control (not mounted, swimming, in furniture, ragdolled...):
	// the sim's buttons are his, not Skyrim's.
	bool OnFoot();
}

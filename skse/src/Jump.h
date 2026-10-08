// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// The jump: libratchet's single jump on the player, Skyrim's own jump switched off.
namespace Jump
{
	// After the game's data has loaded: hooks Skyrim's jump input and the player's update.
	void Install();
	// A save loads or a new game starts: forget any jump in progress.
	void Reset();
}

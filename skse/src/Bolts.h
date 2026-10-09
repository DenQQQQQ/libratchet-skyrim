// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

#include "RatchetDll.h"

// Rift Apart's bolts, the currency (libbolts through ratchet.dll's ratchet_bolts_*), as the
// sandbox runs them (apps/sandbox/src/bolts.rs). Whatever he kills bursts its reward out from
// 1 m up (its worth: its max health over [Bolts] fHealthPerValue, Rift Apart's relative value 1
// to 30, times fLevelMultiplier); the pickups bounce off Skyrim's world, come to rest bobbing and
// spinning, and swarm into him once he (or his thrown wrench) is within 10 m (× fScale), each
// with the game's collect sound. Rift Apart's counter at the top right shows his total: a gain
// waits as "+N" until 2 s after the last, and it's up 3.5 s after a change. The total is kept in
// SKSE's co-save (CoSave).
namespace Bolts
{
	// After the game's data has loaded: the world, and his kills' drops.
	void Install();
	// A load or a new game: the pickups lying about go (his total is the co-save's).
	void Reset();
	// Each frame he's in the world (on foot or not, not in a menu): the drops due, the bolts'
	// step, what reached him, their look and the counter.
	void Update(RE::PlayerCharacter* a_player, float a_dt);

	// His bolts. SetTotal: the co-save's at a load (shown at once, no "+N"), 0 for a new game.
	std::uint32_t Total();
	void          SetTotal(std::uint32_t a_total);

	// For the tests.
	struct Stats
	{
		std::uint32_t dropped = 0;    // bolts burst out so far
		std::uint32_t collected = 0;  // credited to him so far
		std::uint32_t kills = 0;      // his kills that dropped bolts
		std::size_t   queued = 0, flying = 0, resting = 0, pulled = 0;
		float         lowestAboveGround = 0.0f;  // metres: the lowest resting pickup over the ground under it
	};
	Stats Count();
	RatchetTallyView Counter();
}

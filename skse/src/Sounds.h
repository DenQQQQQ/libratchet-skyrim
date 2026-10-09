// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart's sounds on his moves (skse/tools/ratchet_sounds_to_skyrim.py makes them from the
// owner's extraction: WAV files under Data\Sound\FX\RatchetMovement and sounds.txt). A moment
// is the game's sound event: layers that play together, each one of its files at random, at its
// volume, after its delay, at a pitch in its range; the moment's chance is rolled first.
namespace Sounds
{
	// At plugin load: SKSE/Plugins/RatchetMovement/sounds.txt. False: no sounds.
	bool Load(const std::filesystem::path& a_pluginDir);

	// A moment, at a_where (game units), a_after seconds from now. With a_owner, its sounds belong
	// to something (a Shatterbomb in the air): they follow it, and stop with it.
	void Play(std::string_view a_moment, const RE::NiPoint3& a_where, float a_after = 0.0f, std::string_view a_owner = {});
	void Follow(std::string_view a_owner, const RE::NiPoint3& a_where);
	void StopOwned(std::string_view a_owner);
	// A looping moment (the glide's heli, the thrown wrench's whir): played again each time it
	// ends, at a_where, until Stop.
	void Loop(std::string_view a_moment, const RE::NiPoint3& a_where);
	void Stop(std::string_view a_moment);

	// Each frame: delayed layers, and the loops.
	void Update(float a_dt);
}

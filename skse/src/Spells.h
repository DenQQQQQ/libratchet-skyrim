// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Ratchet's hits as Skyrim spells, so Skyrim's own rules follow (the target fights back, crimes,
// kill credit, death). Built at run time from vanilla records in Skyrim.esm:
// - AlchDamageHealth (0003EB42): health damage, here unresisted and delivered to the target;
// - StaggerAttackFFContact (0007809B): a stagger;
// - FireboltProjectile01 (00012E84): the bolt a shot rides (faster than Firebolt's).
// The forms only live for the session (nothing of them is saved: every effect is instant).
namespace Spells
{
	// After the game's data has loaded. False if a vanilla record is missing.
	bool Build();

	// The player hits a_target: a_damage health and a stagger (0..1, 0 none), now. A target not
	// already fighting him takes it as an assault (crime, witnesses) and fights back or flees.
	void Hit(RE::Actor* a_target, float a_damage, float a_stagger);
	void Attacked(RE::PlayerCharacter* a_player, RE::Actor* a_target);

	// The player fires a bolt from a_from (game units) along a_dir (unit), doing a_damage where it
	// lands.
	void Shoot(const RE::NiPoint3& a_from, const RE::NiPoint3& a_dir, float a_damage);
}

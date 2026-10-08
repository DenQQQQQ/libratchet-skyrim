// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Ratchet fights in Skyrim: the OmniWrench (the sim's three-swing combo and its Hyper-Strike)
// and the Burst Pistol, on Skyrim's attack, block and draw buttons (Settings). Hits land on
// Skyrim's actors as spells cast by the player (Spells), so Skyrim's own rules follow.
//
// The pistol's trigger is a small C++ stand-in for the sandbox's libarsenal (which has no C API
// yet): the Burst Pistol's rates, damage and spread from wpn_burstpistol_weapon.json, its shots
// riding a Skyrim bolt.
namespace Combat
{
	// After the game's data has loaded: the hit spells, and Skyrim's own attacks, blocks and
	// weapon drawing switched off on foot.
	void Install();

	// Before the sim steps: the wrench button and aiming into its input. a_view: the camera's
	// forward on the ground (Skyrim's x, y, unit); a_state: the sim's state at its last step.
	void Fill(RatchetInput& a_in, const RE::NiPoint2& a_view, std::uint32_t a_state, float a_dt);

	// After the sim stepped: the wrench's hits, the slam, the gun's shots.
	void Step(RE::PlayerCharacter* a_player, RatchetSim* a_sim, const RatchetStepResult& a_result, float a_dt);

	// For his model.
	enum class Hand
	{
		kNone,
		kWrench,
		kPistol,
	};
	Hand InHand();
	// Aiming, or firing from the hip: he strafes facing the aim.
	bool Strafing();
	// The thrown wrench while it flies: where it is (game units) and how far it has spun
	// (radians, flat).
	std::optional<std::pair<RE::NiPoint3, float>> ThrownWrench();
	// While he strafes: the point the crosshair is on (game units), for his aim pose.
	std::optional<RE::NiPoint3> AimTarget();
	// Shots fired and wrenches thrown so far (his recoil and throw poses restart on a change).
	std::uint32_t Shots();
	std::uint32_t Throws();

	// Tests: the buttons, as if held (nullopt gives them back).
	struct TestButtons
	{
		bool melee = false;
		bool aim = false;
		bool fire = false;
		bool throwWrench = false;
	};
	void SetTestButtons(std::optional<TestButtons> a_buttons);
	// Tests: where the shots go for, instead of where the camera looks (game units).
	void SetTestAim(std::optional<RE::NiPoint3> a_point);
	// What the last hits did (tests read it): actors hit and the damage dealt, since the last call.
	struct Tally
	{
		int   hits = 0;  // the wrench's: swings, slams and throws
		float damage = 0.0f;
		int   shots = 0;
		int   throws = 0;
	};
	Tally TakeTally();
}

// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// libratchet through ratchet.dll (delay-loaded: nothing here may call ratchet_* before
// LoadRatchet() succeeded).
namespace Ratchet
{
	// Loads ratchet.dll from the plugin's folder and checks its API version against ratchet.h.
	bool Load(const std::filesystem::path& a_pluginDir);
	bool Loaded();

	// A new sim at the INI's scale, with the configs loaded if there are any.
	RatchetSim* NewSim();

	// Where a jump on flat ground peaks, from the sim alone: what the sandbox shows at this scale.
	struct JumpNumbers
	{
		float tapApex = 0.0f;       // a one-tick press, metres
		float tapTime = 0.0f;       // seconds to that apex
		float fullApex = 0.0f;      // the button held a_hold seconds (1 s: a full jump)
		float fullTime = 0.0f;
		float fullAirTime = 0.0f;   // takeoff to landing back on the floor
	};
	JumpNumbers MeasureFlatJump(float a_dt, float a_hold = 1.0f);
	const JumpNumbers& Expected();  // measured once at load

	// One frame of a jump as the game ran it: its time step and whether the sim saw the button.
	struct Tick
	{
		float dt;
		bool  jump;
		float moveX = 0.0f;  // the stick in the world (sim axes)
		float moveZ = 0.0f;
		bool  dash = false;
		bool  melee = false;
	};
	struct Replayed
	{
		float apex = 0.0f;      // metres above the takeoff
		float apexTime = 0.0f;  // seconds after the takeoff frame
		float airTime = 0.0f;   // back on the floor
		float distance = 0.0f;  // metres across the ground from the takeoff to the landing
	};
	// The same frames, from the takeoff frame on, through a fresh sim on a flat floor, starting at
	// a_velocity (sim axes): what the sandbox does with exactly this input (after the recorded
	// frames, the button stays up and the stick where it last was). Coming down, the floor is
	// a_landing metres above the takeoff's (where the game's ground caught him).
	Replayed ReplayFlat(const std::vector<Tick>& a_ticks, const RatchetVec3& a_velocity = {}, float a_landing = 0.0f);

	struct DashReplayed
	{
		float distance = 0.0f;  // metres across the ground
		float time = 0.0f;      // seconds until the dash is over
	};
	// A ground dash's frames, from its press on, through a fresh sim on a flat floor: how far
	// and how long it goes before he's back on his feet.
	DashReplayed ReplayDash(const std::vector<Tick>& a_ticks, const RatchetVec3& a_velocity = {});
}

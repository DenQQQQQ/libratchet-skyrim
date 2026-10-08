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
	const JumpNumbers& Expected();  // at 1/60 s, measured once at load
}

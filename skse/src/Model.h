// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Ratchet on the player: his own skinned model (meshes\RatchetMovement\ratchet.nif) under the
// player's third-person 3D, the Dragonborn's geometry hidden, his clips played from the sim's
// state, turned to face where he's going.
namespace Model
{
	// What moves him this frame.
	struct Drive
	{
		bool          simJump = false;  // the sim is moving him (a flight, a dash, a wall, a ledge)
		std::uint32_t simState = 0;     // RATCHET_STATE_*
		std::uint32_t events = 0;       // this frame's RATCHET_EVENT_*
		bool          grounded = true;  // Skyrim's controller is on the ground
		bool          wallRight = false;  // on a wall or in a wall jump: the right-hand clips fit
		std::uint8_t  ledgeMove = 0;    // the move onto a ledge (RatchetStepResult::ledge_move)
		float         verticalSpeed = 0.0f;  // the sim's, m/s (sliding down a wall: below 0)
		// The sim's facing (radians, atan2(x, z) in its axes): he turns the way Rift Apart turns him.
		bool          hasFacing = false;
		float         facingYaw = 0.0f;
	};

	// Reads the clips (at plugin load). False: no model (the files aren't installed).
	bool Load(const std::filesystem::path& a_pluginDir);
	bool Enabled();
	// Each frame after Skyrim has moved the player.
	void Update(RE::PlayerCharacter* a_player, float a_dt, const Drive& a_drive);
	// What clip plays (for logs and tests).
	std::string_view CurrentClip();

	// Tests: hold off attaching him, and show some of his shapes only (-1 all, -2 none, else that
	// one).
	void SetAttachAllowed(bool a_allowed);
	void SetCastShadows(bool a_cast);
	void ShowShapes(int a_only);
	std::vector<std::string> ShapeNames();
}

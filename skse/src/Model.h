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
		std::uint32_t meleeSwing = 0;   // the wrench combo's swing (0 = first)
		bool          strafing = false; // aiming or firing: facing the aim
		float         strafeAngle = 0.0f;  // the way he moves from where he faces, radians (0 ahead, pi/2 right)
		std::optional<RE::NiPoint3> aimAt;  // what the crosshair is on while he strafes (game units)
		std::uint32_t shots = 0;        // shots so far (his recoil restarts on a change)
		std::uint32_t throws = 0;       // throws so far
		// The sim's facing (radians, atan2(x, z) in its axes): he turns the way Rift Apart turns him.
		bool          hasFacing = false;
		float         facingYaw = 0.0f;
		// The sim, for his clips (libratchet-anim reads its state), and how far down the ground
		// was at its last step (metres, if any was found).
		const RatchetSim*    sim = nullptr;
		std::optional<float> groundDistance;
	};

	// Reads the clips (at plugin load). False: no model (the files aren't installed).
	bool Load(const std::filesystem::path& a_pluginDir);
	bool Enabled();
	// Each frame after Skyrim has moved the player.
	void Update(RE::PlayerCharacter* a_player, float a_dt, const Drive& a_drive);
	// After a load: no clip from before it.
	void Reset();
	// His model, Clank, his weapons, the Shatterbomb and its glove loaded once, as a game loads (not
	// on the first frame he's drawn, or the first time a Shatterbomb's out).
	void Preload();
	// His clips sound his steps, efforts, swishes and swings (clip_sounds.txt, on the sandbox's
	// clip clock): the moves' own sounds are then only those the sandbox plays from them.
	bool ClipSounds();
	// What clip plays (for logs and tests); on the wrench or the pistol (their own clips).
	std::string_view CurrentClip();
	std::string_view WeaponClip(bool a_pistol);
	// Where his gun hand is (his igLoc_RT_gunAttach, game units), once his model is posed.
	std::optional<RE::NiPoint3> GunHand();
	// The OmniWrench in his hand (drawn there, not thrown): where it is in the world.
	std::optional<RE::NiTransform> HeldWrench();
	// A locator of the Burst Pistol in his hand (its glTF name: igLoc_bulletEmit,
	// igLoc_shell_eject), in the world, while it's drawn.
	std::optional<RE::NiTransform> PistolLocator(std::string_view a_name);

	// Tests: hold off attaching him, and show some of his shapes only (-1 all, -2 none, else that
	// one).
	void SetAttachAllowed(bool a_allowed);
	void SetCastShadows(bool a_cast);
	void ShowShapes(int a_only);
	std::vector<std::string> ShapeNames();
}

// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Clank on Ratchet's back (meshes\RatchetMovement\clank.nif and clank.anim, from
// skse/tools/ratchet_to_skyrim.py --name clank), hung the way the sandbox and the game hang him
// (apps/sandbox/src/clank.rs): his dyn_backpack_attach joint on Ratchet's ClankAttach bone. He
// plays his backpack idle, and his heli-pack comes out while Ratchet glides (extend, glide,
// retract), its rotors spun in code like the game spins them.
namespace Clank
{
	// Reads his clips (at plugin load). False: he isn't installed or is off in the INI (Ratchet
	// goes alone).
	bool Load(const std::filesystem::path& a_pluginDir);
	bool Enabled();
	// Each frame once Ratchet's joints are posed, before his model's world update. a_ratchet:
	// Ratchet's model.
	void Update(RE::NiNode* a_ratchet, bool a_gliding, float a_dt);
	// What clip plays (for logs and tests), and whether his heli-pack is out.
	std::string_view CurrentClip();
	bool HeliOut();
}

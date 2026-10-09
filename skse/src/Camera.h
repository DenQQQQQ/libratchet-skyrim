// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart's follow camera, as the sandbox places it (apps/sandbox/src/camera.rs and main.rs
// camera_follow) from the game's camera configs: around a pivot above his feet, further back
// and wider while he runs, over his right shoulder while he aims, shaking with his weapons.
// Skyrim's third-person camera keeps its mouse look and its other states (first person, a
// horse, furniture, dialogue): on foot, where it sits and its field of view are Rift Apart's.
namespace Camera
{
	// Reads the camera configs (camera_*.json, camerashake_*.json and the Shatterbomb's shake
	// radii from the configs folder; the sandbox's generic numbers without them) and hooks
	// Skyrim's third-person camera.
	void Install(const std::filesystem::path& a_configs);

	// The Burst Pistol's recoil shake (each shot).
	void Recoil();
	// The Shatterbomb's blast shake: full within its inner radius of him, fading out to its outer
	// (× scale). False if Rift Apart's camera isn't on (Skyrim's own shake can stand in).
	bool Explosion(const RE::NiPoint3& a_at);
	// A short wobble (the Hyper-Strike's landing; the sandbox's own).
	void Wobble(float a_seconds);
}

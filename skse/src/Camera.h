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

	// Once a frame: outside Skyrim's third-person camera on foot (first person, a horse,
	// furniture), Skyrim's own field of view and no shakes.
	void Update();

	// The Burst Pistol's recoil shake (each shot).
	void Recoil();
	// The Shatterbomb's blast shake: full within its inner radius of him, fading out to its outer
	// (× scale). False if Rift Apart's camera isn't on (Skyrim's own shake can stand in).
	bool Explosion(const RE::NiPoint3& a_at);
	// A short wobble (the Hyper-Strike's landing; the sandbox's own).
	void Wobble(float a_seconds);

	// Where Rift Apart's camera looks without its shakes (Skyrim's mouse look): the stick and the
	// aim turn by it (the sandbox shakes only the drawn camera, not its rig). nullopt when it's off.
	std::optional<RE::NiPoint3> LookForward();

	// Tests: the wheel's zoom (metres back standing, the world's), and a notch of the wheel sent
	// through Skyrim's third-person camera's own input handler (a_in: Zoom In).
	float ZoomDistance();
	void  TestWheel(bool a_in);
}

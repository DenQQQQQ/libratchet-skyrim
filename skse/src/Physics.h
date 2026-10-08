// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rays through the player's Havok world.
namespace Physics
{
	struct Hit
	{
		RE::NiPoint3       point;   // game units
		RE::NiPoint3       normal;
		float              fraction;  // along the ray, 0..1
		RE::TESObjectREFR* ref = nullptr;  // what was hit, if it's a reference (an actor, a door...)
	};

	// The first thing the ray hits that the player would collide with (his own body ignored).
	// Game units.
	std::optional<Hit> Ray(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to);

	// Metres from his feet down to the ground (looking up to a_max metres below); nullopt if
	// there's none that close.
	std::optional<float> GroundDistance(const RE::NiPoint3& a_feet, float a_max);

	struct Wall
	{
		RE::NiPoint2 normal;  // out of the wall, on the ground plane (Skyrim's x, y), unit length
		float        distance;  // metres from his middle to the wall's face, along the normal
	};
	// The nearest wall beside him as Rift Apart's wall probe finds one (the sandbox's
	// level::wall_probe): a steep face within a_reach metres of his middle (a_middle metres above
	// his feet) that also reaches a_side metres past him to either side. A face along a_prefer
	// (the wall he's on) wins over nearer ones.
	std::optional<Wall> WallProbe(const RE::NiPoint3& a_feet, float a_middle, float a_reach, float a_side, std::optional<RE::NiPoint2> a_prefer);
}

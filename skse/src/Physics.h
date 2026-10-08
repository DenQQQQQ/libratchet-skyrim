// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rays through the player's Havok world.
namespace Physics
{
	struct Hit
	{
		RE::NiPoint3 point;   // game units
		RE::NiPoint3 normal;
		float        fraction;  // along the ray, 0..1
	};

	// The first thing the ray hits that the player would collide with (his own body ignored).
	// Game units.
	std::optional<Hit> Ray(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to);

	// Metres from his feet down to the ground (looking up to a_max metres below); nullopt if
	// there's none that close.
	std::optional<float> GroundDistance(const RE::NiPoint3& a_feet, float a_max);
}

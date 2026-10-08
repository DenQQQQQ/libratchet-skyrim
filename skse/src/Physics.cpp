// SPDX-License-Identifier: GPL-3.0-or-later
#include "Physics.h"

namespace Physics
{
	std::optional<Hit> Ray(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		auto* cell = player ? player->GetParentCell() : nullptr;
		auto* world = cell ? cell->GetbhkWorld() : nullptr;
		if (!world) {
			return std::nullopt;
		}
		const float s = RE::bhkWorld::GetWorldScale();
		RE::bhkPickData pick{};
		pick.rayInput.from = RE::hkVector4(a_from.x * s, a_from.y * s, a_from.z * s, 0.0f);
		pick.rayInput.to = RE::hkVector4(a_to.x * s, a_to.y * s, a_to.z * s, 0.0f);
		// What his character controller collides with, in his own collision group (so the ray
		// passes through his own capsule).
		RE::CFilter filter{};
		player->GetCollisionFilterInfo(filter);
		pick.rayInput.filterInfo.filter = (filter.filter & 0xFFFF0000u) | static_cast<std::uint32_t>(RE::COL_LAYER::kCharController);
		{
			RE::BSReadLockGuard lock(world->worldLock);
			world->PickObject(pick);
		}
		if (!pick.rayOutput.HasHit()) {
			return std::nullopt;
		}
		const float f = pick.rayOutput.hitFraction;
		const auto& n = pick.rayOutput.normal.quad.m128_f32;
		return Hit{ a_from + (a_to - a_from) * f, { n[0], n[1], n[2] }, f };
	}

	std::optional<float> GroundDistance(const RE::NiPoint3& a_feet, float a_max)
	{
		const float units = RE::bhkWorld::GetWorldScaleInverse();
		// From a little above the feet, so a foot just in the ground still finds it.
		const float lift = 0.1f * units;
		const RE::NiPoint3 from = a_feet + RE::NiPoint3{ 0.0f, 0.0f, lift };
		const RE::NiPoint3 to = a_feet - RE::NiPoint3{ 0.0f, 0.0f, a_max * units };
		const auto hit = Ray(from, to);
		if (!hit) {
			return std::nullopt;
		}
		return (from.z - hit->point.z - lift) / units;
	}
}

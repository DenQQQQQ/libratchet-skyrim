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
		RE::TESObjectREFR* ref = pick.rayOutput.rootCollidable ? RE::TESHavokUtilities::FindCollidableRef(*pick.rayOutput.rootCollidable) : nullptr;
		return Hit{ a_from + (a_to - a_from) * f, { n[0], n[1], n[2] }, f, ref };
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

	std::optional<Wall> WallProbe(const RE::NiPoint3& a_feet, float a_middle, float a_reach, float a_side, std::optional<RE::NiPoint2> a_prefer)
	{
		const float units = RE::bhkWorld::GetWorldScaleInverse();
		const RE::NiPoint3 middle = a_feet + RE::NiPoint3{ 0.0f, 0.0f, a_middle * units };
		// A face hit from his middle: steep enough to be a wall, and its distance along its own
		// normal (a ray at an angle to it travels further than that).
		const auto face = [&](const Hit& a_hit) -> std::optional<Wall> {
			const float len = std::sqrt(a_hit.normal.x * a_hit.normal.x + a_hit.normal.y * a_hit.normal.y);
			if (len < 0.7f) {
				return std::nullopt;  // a floor, a slope or a ceiling
			}
			const RE::NiPoint2 n{ a_hit.normal.x / len, a_hit.normal.y / len };
			const float d = ((middle.x - a_hit.point.x) * n.x + (middle.y - a_hit.point.y) * n.y) / units;
			if (d < -0.01f || d > a_reach) {
				return std::nullopt;
			}
			return Wall{ n, std::max(0.0f, d) };
		};
		std::vector<Wall> found;
		constexpr int kRays = 16;
		for (int i = 0; i < kRays; ++i) {
			const float a = static_cast<float>(i) * (2.0f * 3.14159265f / kRays);
			const RE::NiPoint3 dir{ std::cos(a), std::sin(a), 0.0f };
			if (const auto hit = Ray(middle, middle + dir * (a_reach * 1.5f * units))) {
				if (const auto wall = face(*hit)) {
					found.push_back(*wall);
				}
			}
		}
		const auto preferred = [&](const Wall& a_wall) {
			return a_prefer && a_wall.normal.x * a_prefer->x + a_wall.normal.y * a_prefer->y > 0.9f;
		};
		std::ranges::sort(found, [&](const Wall& a, const Wall& b) {
			return std::pair(!preferred(a), a.distance) < std::pair(!preferred(b), b.distance);
		});
		// The game probes three times: straight at the wall and a_side to either side. The face
		// has to be there for all three.
		for (const Wall& wall : found) {
			const RE::NiPoint3 into{ -wall.normal.x, -wall.normal.y, 0.0f };
			const RE::NiPoint3 along{ -wall.normal.y, wall.normal.x, 0.0f };
			bool wide = true;
			for (const float side : { a_side, -a_side }) {
				const RE::NiPoint3 from = middle + along * (side * units);
				const auto hit = Ray(from, from + into * ((a_reach + 0.3f) * units));
				const auto there = hit ? face(*hit) : std::nullopt;
				if (!there || there->normal.x * wall.normal.x + there->normal.y * wall.normal.y < 0.8f) {
					wide = false;
					break;
				}
			}
			if (wide) {
				return wall;
			}
		}
		return std::nullopt;
	}
}

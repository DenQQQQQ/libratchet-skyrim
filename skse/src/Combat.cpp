// SPDX-License-Identifier: GPL-3.0-or-later
#include "Combat.h"

#include "Input.h"
#include "Jump.h"
#include "Model.h"
#include "Physics.h"
#include "Settings.h"
#include "Spells.h"

namespace Combat
{
	namespace
	{
		// The sandbox's swing reach (apps/sandbox/src/combat.rs melee_hits): within 1.8 m × scale
		// of him and in front (cosine above 0.2). Rift Apart's own hit volume isn't known.
		constexpr float kSwingReach = 1.8f;
		constexpr float kSwingCone = 0.2f;
		// How far above or below him a hit still lands (ours), × scale.
		constexpr float kHitHeight = 1.5f;

		// The Burst Pistol (wpn_burstpistol_weapon.json, as the sandbox reads it): 11.36 damage a
		// shot, 3 shots/s aimed, 6 from the hip (kMisc3, the sandbox's guess), out to 45 m. Spread:
		// from the hip 0.1° growing 0.05° a shot after the third, up to 4°, back at 25°/s; aimed
		// 0.1° growing 0.01°, up to 0.65°, back at 5°/s.
		constexpr float kShotDamage = 11.36f;
		constexpr float kAimedRate = 3.0f;
		constexpr float kHipRate = 6.0f;
		constexpr float kRange = 45.0f;
		// Firing from the hip, he keeps strafing this long after the last shot (the sandbox's).
		constexpr float kStrafeAfterShot = 0.1f;

		// The thrown OmniWrench (wpn_wrench_weapon.json ThrowDamageInfo; the throw clip's
		// WeaponThrown event): 30 damage to anyone within 0.5 m (× scale) of its path, once a
		// throw, leaving his hand 0.068 s after the press. The sim flies it (ratchet_wrench_*).
		constexpr float kThrowDamage = 30.0f;
		constexpr float kThrowRadius = 0.5f;
		constexpr float kThrowRelease = 0.068f;
		// How wide an actor is around its middle, for the throw's reach (ours).
		constexpr float kBodyRadius = 0.4f;

		struct Spread
		{
			float min, grow, max, recover;  // degrees, degrees a shot, degrees, degrees/s
		};
		constexpr Spread kHipSpread{ 0.1f, 0.05f, 4.0f, 25.0f };
		constexpr Spread kAimedSpread{ 0.1f, 0.01f, 0.65f, 5.0f };

		struct State
		{
			std::optional<TestButtons> test;
			std::optional<RE::NiPoint3> testAim;
			bool          melee = false;
			bool          aim = false;
			bool          fire = false;
			std::uint32_t simState = 0;
			float         sinceShot = 100.0f;
			float         sinceWrench = 100.0f;
			float         cooldown = 0.0f;
			float         spread = kHipSpread.min;
			int           shotsInARow = 0;
			std::uint32_t random = 0x9E3779B9u;
			Tally         tally;
			// The throw.
			bool           throwHeld = false;   // the button, for its press
			float          releaseIn = -1.0f;   // winding up: seconds until it leaves his hand
			RE::NiPoint2   throwView;           // the camera's flat forward at the press
			RatchetWrench* thrown = nullptr;    // in the air
			RE::NiPoint3   wrenchAt;            // where it is, game units
			float          wrenchSpin = 0.0f;
			std::uint8_t   blocked = 0;         // it hit a wall since its last step
			std::vector<RE::ActorHandle> struck;  // hit by this throw already
		};
		State c;

		RatchetVec3 ToSim(const RE::NiPoint3& a_units)
		{
			const float k = 1.0f / RE::bhkWorld::GetWorldScaleInverse();
			return { a_units.x * k, a_units.z * k, -a_units.y * k };
		}

		RE::NiPoint3 FromSim(const RatchetVec3& a_metres)
		{
			const float u = RE::bhkWorld::GetWorldScaleInverse();
			return { a_metres.x * u, -a_metres.z * u, a_metres.y * u };
		}

		float Random01()
		{
			// xorshift: the spread needs no more.
			c.random ^= c.random << 13;
			c.random ^= c.random >> 17;
			c.random ^= c.random << 5;
			return static_cast<float>(c.random >> 8) / 16777216.0f;
		}

		float Units() { return RE::bhkWorld::GetWorldScaleInverse(); }

		// No firing while dashing, swinging, on a wall or a ledge, or swinging on a target (the
		// sandbox's rule: states 5 to 18).
		bool CanFire(std::uint32_t a_state)
		{
			return a_state <= RATCHET_STATE_GLIDE;
		}

		template <class F>
		void ForEachTarget(RE::PlayerCharacter* a_player, F&& a_fn)
		{
			auto* lists = RE::ProcessLists::GetSingleton();
			if (!lists) {
				return;
			}
			lists->ForEachHighActor([&](RE::Actor* a_actor) {
				if (a_actor && a_actor != a_player && !a_actor->IsDead() && !a_actor->IsPlayerTeammate() && a_actor->Get3D()) {
					a_fn(a_actor);
				}
				return RE::BSContainer::ForEachResult::kContinue;
			});
		}

		void Deal(RE::Actor* a_target, float a_damage, float a_stagger)
		{
			const float damage = a_damage * Settings::Get().damageMult;
			Spells::Hit(a_target, damage, a_stagger);
			++c.tally.hits;
			c.tally.damage += damage;
		}

		// A swing connects now: everyone in reach in front of him.
		void Swing(RE::PlayerCharacter* a_player, RatchetSim* a_sim, const RatchetStepResult& a_r)
		{
			RatchetSwingInfo info{};
			if (ratchet_swing_info(a_sim, a_r.melee_swing, &info) == 0) {
				return;
			}
			const float s = Settings::Get().scale;
			const float u = Units();
			const RE::NiPoint3 me = a_player->GetPosition();
			// The sim's facing (atan2(x, z) in its axes, -z north) on Skyrim's ground plane.
			const RE::NiPoint2 ahead{ std::sin(a_r.facing_yaw), -std::cos(a_r.facing_yaw) };
			// The last swing knocks back furthest (4 m): a full stagger; the others half.
			const float stagger = info.knockback >= 3.0f * s ? 1.0f : 0.5f;
			int hits = 0;
			ForEachTarget(a_player, [&](RE::Actor* a_target) {
				const RE::NiPoint3 d = a_target->GetPosition() - me;
				const float across = std::sqrt(d.x * d.x + d.y * d.y) / u;
				if (across > kSwingReach * s || std::abs(d.z) / u > kHitHeight * s) {
					return;
				}
				if (across > 0.3f && (d.x * ahead.x + d.y * ahead.y) / (across * u) < kSwingCone) {
					return;
				}
				Deal(a_target, info.damage, stagger);
				++hits;
			});
			SKSE::log::info("combat: swing {} hits {} ({:.1f} damage each)", a_r.melee_swing + 1, hits, info.damage * Settings::Get().damageMult);
		}

		// The Hyper-Strike lands: everyone around him.
		void Slam(RE::PlayerCharacter* a_player, RatchetSim* a_sim)
		{
			float damage = 0.0f, radius = 0.0f;
			ratchet_smash_info(a_sim, &damage, &radius);
			const float s = Settings::Get().scale;
			const float u = Units();
			const RE::NiPoint3 me = a_player->GetPosition();
			int hits = 0;
			ForEachTarget(a_player, [&](RE::Actor* a_target) {
				const RE::NiPoint3 d = a_target->GetPosition() - me;
				if (std::sqrt(d.x * d.x + d.y * d.y) / u > radius || std::abs(d.z) / u > kHitHeight * s) {
					return;
				}
				Deal(a_target, damage, 1.0f);
				++hits;
			});
			SKSE::log::info("combat: slam hits {} ({:.1f} damage within {:.1f} m)", hits, damage * Settings::Get().damageMult, radius);
		}

		// Where the camera looks, as far as kRange × scale: the point the shot goes for.
		RE::NiPoint3 AimPoint()
		{
			if (c.testAim) {
				return *c.testAim;
			}
			const float reach = kRange * Settings::Get().scale * Units();
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!camera || !camera->cameraRoot) {
				auto* player = RE::PlayerCharacter::GetSingleton();
				const float yaw = player->GetAngleZ();
				return player->GetPosition() + RE::NiPoint3{ std::sin(yaw), std::cos(yaw), 0.0f } * reach;
			}
			const RE::NiPoint3 from = camera->cameraRoot->world.translate;
			const RE::NiPoint3 to = from + camera->cameraRoot->world.rotate.GetVectorY() * reach;
			const auto hit = Physics::Ray(from, to);
			return hit ? hit->point : to;
		}

		// A direction within a_degrees of a_dir, uniformly over the cone's disc.
		RE::NiPoint3 Scatter(const RE::NiPoint3& a_dir, float a_degrees)
		{
			const float r = a_degrees * (3.14159265f / 180.0f) * std::sqrt(Random01());
			const float a = 2.0f * 3.14159265f * Random01();
			const RE::NiPoint3 side = std::abs(a_dir.z) < 0.9f ? a_dir.UnitCross({ 0.0f, 0.0f, 1.0f }) : a_dir.UnitCross({ 1.0f, 0.0f, 0.0f });
			const RE::NiPoint3 up = side.Cross(a_dir);
			RE::NiPoint3 d = a_dir + side * (std::tan(r) * std::cos(a)) + up * (std::tan(r) * std::sin(a));
			d.Unitize();
			return d;
		}

		// His gun hand (his model's), or near where it would be.
		RE::NiPoint3 HandAt(RE::PlayerCharacter* a_player)
		{
			if (const auto hand = Model::GunHand()) {
				return *hand;
			}
			const float s = Settings::Get().scale;
			const float u = Units();
			const float yaw = a_player->GetAngleZ();
			return a_player->GetPosition() + RE::NiPoint3{ std::sin(yaw) * 0.4f * s * u, std::cos(yaw) * 0.4f * s * u, 1.1f * s * u };
		}

		// The wrench leaves his hand, flat along where the camera looked at the press (the
		// sandbox's way).
		void Release(RE::PlayerCharacter* a_player, RatchetSim* a_sim)
		{
			const RE::NiPoint3 hand = HandAt(a_player);
			c.thrown = ratchet_wrench_throw(a_sim, ToSim(hand), RatchetVec3{ c.throwView.x, 0.0f, -c.throwView.y });
			c.wrenchAt = hand;
			c.wrenchSpin = 0.0f;
			c.blocked = 0;
			c.struck.clear();
			++c.tally.throws;
		}

		// Everyone within reach of its path from a_from to a_to takes the throw's hit, once.
		void Strike(RE::PlayerCharacter* a_player, const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
		{
			const float s = Settings::Get().scale;
			const float u = Units();
			const float reach = (kThrowRadius + kBodyRadius) * s * u;
			const RE::NiPoint3 path = a_to - a_from;
			const float len2 = path.Dot(path);
			ForEachTarget(a_player, [&](RE::Actor* a_target) {
				const auto handle = a_target->GetHandle();
				if (std::ranges::find(c.struck, handle) != c.struck.end()) {
					return;
				}
				const float half = a_target->GetHeight() * 0.5f;
				const RE::NiPoint3 middle = a_target->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, half };
				const float t = len2 > 0.0f ? std::clamp((middle - a_from).Dot(path) / len2, 0.0f, 1.0f) : 0.0f;
				const RE::NiPoint3 d = middle - (a_from + path * t);
				if (std::sqrt(d.x * d.x + d.y * d.y) > reach || std::abs(d.z) > half + kThrowRadius * s * u) {
					return;
				}
				c.struck.push_back(handle);
				Deal(a_target, kThrowDamage, 0.5f);
				SKSE::log::info("combat: the thrown wrench hits");
			});
		}

		void Fly(RE::PlayerCharacter* a_player, float a_dt)
		{
			RatchetVec3 pos{};
			float       spin = 0.0f;
			const bool  caught = ratchet_wrench_step(c.thrown, a_dt, ToSim(HandAt(a_player)), c.blocked, &pos, &spin) != 0;
			const RE::NiPoint3 now = FromSim(pos);
			c.blocked = 0;
			if (!caught) {
				// A wall turns it back; actors don't (it flies on through, hitting them).
				const auto hit = Physics::Ray(c.wrenchAt, now);
				if (hit && !(hit->ref && hit->ref->As<RE::Actor>())) {
					c.blocked = 1;
				}
				Strike(a_player, c.wrenchAt, now);
			}
			c.wrenchAt = now;
			c.wrenchSpin = spin;
			if (caught) {
				ratchet_wrench_free(c.thrown);
				c.thrown = nullptr;
				c.struck.clear();
			}
		}

		void Gun(RE::PlayerCharacter* a_player, float a_dt)
		{
			const Spread& spread = c.aim ? kAimedSpread : kHipSpread;
			c.cooldown = std::max(0.0f, c.cooldown - a_dt);
			if (!c.fire || !CanFire(c.simState) || c.thrown || c.releaseIn >= 0.0f) {
				c.spread = std::max(spread.min, c.spread - spread.recover * a_dt);
				c.shotsInARow = 0;
				return;
			}
			if (c.cooldown > 0.0f) {
				return;
			}
			c.cooldown = 1.0f / (c.aim ? kAimedRate : kHipRate);
			if (++c.shotsInARow > 3) {
				c.spread = std::min(spread.max, c.spread + spread.grow);
			}
			c.spread = std::clamp(c.spread, spread.min, spread.max);
			const RE::NiPoint3 muzzle = HandAt(a_player);
			RE::NiPoint3       dir = AimPoint() - muzzle;
			if (dir.Length() < 1.0f) {
				return;
			}
			dir.Unitize();
			Spells::Shoot(muzzle, Scatter(dir, c.spread), kShotDamage * Settings::Get().damageMult);
			c.sinceShot = 0.0f;
			++c.tally.shots;
		}

		// --- Skyrim's own fighting, off on foot

		template <class Handler>
		struct OffOnFoot
		{
			static bool CanProcess(Handler* a_this, RE::InputEvent* a_event)
			{
				if (Jump::OnFoot()) {
					return false;
				}
				return _CanProcess(a_this, a_event);
			}
			static inline REL::Relocation<decltype(CanProcess)> _CanProcess;

			static void Install(REL::VariantID a_vtable)
			{
				REL::Relocation<std::uintptr_t> vtbl{ a_vtable };
				_CanProcess = vtbl.write_vfunc(0x1, CanProcess);
			}
		};

		bool Uses(std::string_view a_control)
		{
			const auto& s = Settings::Get();
			return s.fireControl == a_control || s.aimControl == a_control || s.meleeControl == a_control || s.throwControl == a_control;
		}
	}

	void Install()
	{
		if (!Settings::Get().combat) {
			return;
		}
		Spells::Build();
		std::string off;
		if (Uses("Right Attack/Block") || Uses("Left Attack/Block")) {
			OffOnFoot<RE::AttackBlockHandler>::Install(RE::VTABLE_AttackBlockHandler[0]);
			off += " attacks and blocks";
		}
		if (Uses("Ready Weapon")) {
			OffOnFoot<RE::ReadyWeaponHandler>::Install(RE::VTABLE_ReadyWeaponHandler[0]);
			off += off.empty() ? " weapon drawing" : ", weapon drawing";
		}
		if (Uses("Shout")) {
			OffOnFoot<RE::ShoutHandler>::Install(RE::VTABLE_ShoutHandler[0]);
			off += off.empty() ? " shouts" : ", shouts";
		}
		const auto& s = Settings::Get();
		SKSE::log::info("combat: fire on {}, aim on {}, wrench on {}, throw on {}; Skyrim's own{} are off on foot", s.fireControl,
			s.aimControl, s.meleeControl, s.throwControl, off);
	}

	void Fill(RatchetInput& a_in, const RE::NiPoint2& a_view, std::uint32_t a_state, float a_dt)
	{
		const auto& s = Settings::Get();
		c.simState = a_state;
		c.sinceShot += a_dt;
		if (!s.combat) {
			return;
		}
		const TestButtons t = c.test.value_or(TestButtons{});
		c.melee = t.melee || Input::ControlHeld(s.meleeControl);
		c.aim = t.aim || Input::ControlHeld(s.aimControl);
		c.fire = t.fire || Input::ControlHeld(s.fireControl);
		// The throw: on its press, unless the wrench is out already or he's busy (dashing,
		// swinging, on a wall or ledge).
		const bool throwHeld = t.throwWrench || Input::ControlHeld(s.throwControl);
		if (throwHeld && !c.throwHeld && !c.thrown && c.releaseIn < 0.0f && CanFire(a_state)) {
			c.releaseIn = kThrowRelease;
			c.throwView = a_view;
		}
		c.throwHeld = throwHeld;
		// No swings while aiming, or while the wrench is out of his hand (the sandbox's rule).
		a_in.melee = c.melee && !c.aim && !c.thrown && c.releaseIn < 0.0f ? 1 : 0;
		if (c.aim) {
			a_in.aiming = 1;
		} else if ((c.fire && CanFire(a_state)) || c.sinceShot < kStrafeAfterShot) {
			a_in.aiming = 2;
		}
		if (a_in.aiming) {
			a_in.aim_x = a_view.x;
			a_in.aim_z = -a_view.y;
		}
	}

	void Step(RE::PlayerCharacter* a_player, RatchetSim* a_sim, const RatchetStepResult& a_r, float a_dt)
	{
		if (!Settings::Get().combat || !a_player || !a_sim) {
			return;
		}
		c.simState = a_r.state;
		const bool wrench = a_r.state == RATCHET_STATE_MELEE || a_r.state == RATCHET_STATE_SMASH || a_r.state == RATCHET_STATE_SMASH_LAND;
		c.sinceWrench = wrench ? 0.0f : c.sinceWrench + a_dt;
		if (a_r.events & RATCHET_EVENT_MELEE_HIT) {
			Swing(a_player, a_sim, a_r);
		}
		if (a_r.events & RATCHET_EVENT_SMASH_IMPACT) {
			Slam(a_player, a_sim);
		}
		if (c.releaseIn >= 0.0f) {
			c.releaseIn -= a_dt;
			if (c.releaseIn < 0.0f) {
				Release(a_player, a_sim);
			}
		}
		if (c.thrown) {
			Fly(a_player, a_dt);
		}
		Gun(a_player, a_dt);
	}

	std::optional<std::pair<RE::NiPoint3, float>> ThrownWrench()
	{
		if (!c.thrown) {
			return std::nullopt;
		}
		return std::pair{ c.wrenchAt, c.wrenchSpin };
	}

	Hand InHand()
	{
		if (c.thrown) {
			return Hand::kNone;  // it's flying
		}
		if (c.sinceWrench < 0.5f || c.releaseIn >= 0.0f) {
			return Hand::kWrench;
		}
		if (c.aim || c.sinceShot < 2.0f) {
			return Hand::kPistol;
		}
		return Hand::kNone;
	}

	bool Strafing()
	{
		return Settings::Get().combat && (c.aim || ((c.fire && CanFire(c.simState)) || c.sinceShot < kStrafeAfterShot));
	}

	void SetTestButtons(std::optional<TestButtons> a_buttons) { c.test = a_buttons; }

	void SetTestAim(std::optional<RE::NiPoint3> a_point) { c.testAim = a_point; }

	Tally TakeTally()
	{
		return std::exchange(c.tally, Tally{});
	}
}

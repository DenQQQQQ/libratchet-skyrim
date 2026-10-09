// SPDX-License-Identifier: GPL-3.0-or-later
#include "Combat.h"

#include "AutoTest.h"
#include "Input.h"
#include "Jump.h"
#include "Model.h"
#include "Physics.h"
#include "Settings.h"
#include "Sounds.h"
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

		// The Shatterbomb (wpn_protogrenade_weapon.json, as libarsenal reads it: its level-1
		// upgrades win). A cooked throw: holding fire winds it up, letting go throws, 0.8 a second,
		// 6 in the glove (filled again 1.5 s after the last, the sandbox's convenience). Lobbed at
		// 30 m/s under 30 m/s² to come down on the aimed point, aimed 30 m out at most, 8 s in the
		// air at most. Its blast: 170.5 damage within 1 m, falling to 85.25 at 6 m; knocked back
		// 3.5 m within 3 m, 1.5 m further out. Distances and speeds × scale. It leaves his hand
		// 0.1 s into the throw (the sandbox's, without the clip's event).
		constexpr float kBombRate = 0.8f;
		constexpr int   kBombAmmo = 6;
		constexpr float kBombRefill = 1.5f;
		constexpr float kBombSpeed = 30.0f;
		constexpr float kBombGravity = 30.0f;
		constexpr float kBombRange = 30.0f;
		constexpr float kBombLife = 8.0f;
		constexpr float kBombSpin = 500.0f;  // degrees/s, for its look
		constexpr float kBombRelease = 0.1f;
		constexpr float kBlastInner = 1.0f;
		constexpr float kBlastOuter = 6.0f;
		constexpr float kBlastInnerDamage = 170.5f;
		constexpr float kBlastOuterDamage = 85.25f;
		constexpr float kBlastHardPush = 3.0f;  // metres: the strongest knockback tier, a full stagger
		// The gun counts as ready this close to the end of its wait (libarsenal's, float rounding).
		constexpr float kReady = 1e-5f;

		struct Bomb
		{
			RE::NiPoint3 at;        // game units
			RE::NiPoint3 velocity;  // m/s, Skyrim's axes
			float        age = 0.0f;
			float        spin = 0.0f;
		};

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
			bool           turned = false;      // a wall turned it back (logged once)
			RE::NiPoint3   thrownFrom;
			float          thrownFor = 0.0f;    // seconds out
			float          thrownFar = 0.0f;    // metres out at most
			std::vector<RE::ActorHandle> struck;  // hit by this throw already
			// The guns: which is out, and the Shatterbomb's trigger as libarsenal's Weapon keeps it.
			Gun               gun = Gun::kBurstPistol;
			bool              pickHeld[2] = {};
			bool              bombTrigger = false;  // the trigger at its last step (the throw goes on its release)
			float             bombCooldown = 0.0f;
			int               bombAmmo = kBombAmmo;
			float             bombEmpty = 0.0f;     // seconds the glove has been empty
			float             bombIn = -1.0f;       // a throw: seconds until the bomb leaves his hand
			bool              cooking = false;
			std::vector<Bomb> bombs;
			std::uint32_t     bombThrows = 0;
			float             sinceBlast = 100.0f;
			// For his poses.
			std::optional<RE::NiPoint3> aimAt;
			std::uint32_t               shots = 0;
			std::uint32_t               throws = 0;
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

		// The point of a_target's body nearest a_from: actors are long (a cow is 2.5 m nose to
		// tail, a horse or a dragon more), and a hit counts to their body, not their middle (a
		// lunge along a cow's side missed it). The body: a segment along their heading, as long as
		// their bounds less their width, at the bounds' middle.
		RE::NiPoint3 NearestOfBody(RE::Actor* a_target, const RE::NiPoint3& a_from)
		{
			const RE::NiPoint3 lo = a_target->GetBoundMin(), hi = a_target->GetBoundMax();
			const float        scale = a_target->GetScale();
			const float        half = std::max(0.0f, ((hi.y - lo.y) - (hi.x - lo.x)) * 0.5f * scale);
			const float        heading = a_target->GetAngleZ();
			const RE::NiPoint3 axis{ std::sin(heading), std::cos(heading), 0.0f };
			const RE::NiPoint3 middle = a_target->GetPosition() + axis * ((hi.y + lo.y) * 0.5f * scale);
			const float        t = std::clamp((a_from - middle).Dot(axis), -half, half);
			return middle + axis * t;
		}

		void Deal(RE::Actor* a_target, float a_damage, float a_stagger)
		{
			const float damage = a_damage * Settings::Get().damageMult;
			Spells::Hit(a_target, damage, a_stagger);
			Sounds::Play("wrench_hit", a_target->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, a_target->GetHeight() * 0.5f });
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
				const RE::NiPoint3 d = NearestOfBody(a_target, me) - me;
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
				const RE::NiPoint3 d = NearestOfBody(a_target, me) - me;
				if (std::sqrt(d.x * d.x + d.y * d.y) / u > radius || std::abs(d.z) / u > kHitHeight * s) {
					return;
				}
				Deal(a_target, damage, 1.0f);
				++hits;
			});
			SKSE::log::info("combat: slam hits {} ({:.1f} damage within {:.1f} m)", hits, damage * Settings::Get().damageMult, radius);
		}

		// Where the camera looks, as far as kRange × scale: the point the shot goes for, and
		// whether something is there.
		std::pair<RE::NiPoint3, bool> AimPoint()
		{
			if (c.testAim) {
				return { *c.testAim, true };
			}
			const float reach = kRange * Settings::Get().scale * Units();
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!camera || !camera->cameraRoot) {
				auto* player = RE::PlayerCharacter::GetSingleton();
				const float yaw = player->GetAngleZ();
				return { player->GetPosition() + RE::NiPoint3{ std::sin(yaw), std::cos(yaw), 0.0f } * reach, false };
			}
			const RE::NiPoint3 from = camera->cameraRoot->world.translate;
			const RE::NiPoint3 to = from + camera->cameraRoot->world.rotate.GetVectorY() * reach;
			const auto hit = Physics::Ray(from, to);
			return hit ? std::pair{ hit->point, true } : std::pair{ to, false };
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
			RE::NiPoint2       dir = c.throwView;
			if (c.testAim) {
				// Tests: at their target, flat.
				const RE::NiPoint2 d{ c.testAim->x - hand.x, c.testAim->y - hand.y };
				const float        len = std::sqrt(d.x * d.x + d.y * d.y);
				if (len > 1.0f) {
					dir = { d.x / len, d.y / len };
				}
			}
			c.thrown = ratchet_wrench_throw(a_sim, ToSim(hand), RatchetVec3{ dir.x, 0.0f, -dir.y });
			c.wrenchAt = hand;
			c.wrenchSpin = 0.0f;
			c.blocked = 0;
			c.turned = false;
			c.thrownFrom = hand;
			c.thrownFor = 0.0f;
			c.thrownFar = 0.0f;
			c.struck.clear();
			++c.tally.throws;
			++c.throws;
			Sounds::Play("wrench_throw", hand);
			Sounds::Play("voice_throw", hand);
			SKSE::log::info("combat: the wrench leaves his hand ({})", c.thrown ? "flying" : "the sim didn't throw it");
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
				const RE::NiPoint3 middle = NearestOfBody(a_target, a_from + path * 0.5f) + RE::NiPoint3{ 0.0f, 0.0f, half };
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
			if (c.thrownFor < 0.15f && c.thrownFor + a_dt >= 0.15f) {
				AutoTest::OnMove("wrench_flying");
			}
			c.thrownFor += a_dt;
			c.thrownFar = std::max(c.thrownFar, now.GetDistance(c.thrownFrom) / Units());
			if (!caught) {
				// A wall turns it back; actors don't (it flies on through, hitting them).
				const auto hit = Physics::Ray(c.wrenchAt, now);
				if (hit && !(hit->ref && hit->ref->As<RE::Actor>())) {
					c.blocked = 1;
					if (!c.turned) {
						c.turned = true;
						SKSE::log::info("combat: the wrench hit {} {:.1f} m out and turns back", hit->ref ? hit->ref->GetName() : "the world",
							hit->point.GetDistance(c.thrownFrom) / Units());
					}
				}
				Strike(a_player, c.wrenchAt, now);
			}
			c.wrenchAt = now;
			c.wrenchSpin = spin;
			if (caught) {
				ratchet_wrench_free(c.thrown);
				c.thrown = nullptr;
				c.struck.clear();
				Sounds::Stop("wrench_throw_loop");
				Sounds::Play("wrench_catch", now);
				SKSE::log::info("combat: the wrench is back after {:.2f} s, {:.1f} m out at most", c.thrownFor, c.thrownFar);
			} else {
				Sounds::Loop("wrench_throw_loop", now);
			}
		}

		// Launch velocity at a_speed that comes down on a_to under a_gravity: the lower of the two
		// arcs; out of reach, 45° (the longest throw) toward it (libarsenal's lob_velocity).
		// Metres and m/s, Skyrim's axes (z up).
		RE::NiPoint3 LobVelocity(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to, float a_speed, float a_gravity)
		{
			const RE::NiPoint3 d = a_to - a_from;
			const float        flat = std::sqrt(d.x * d.x + d.y * d.y);
			const RE::NiPoint2 dir = flat > 1e-4f ? RE::NiPoint2{ d.x / flat, d.y / flat } : RE::NiPoint2{ 0.0f, 1.0f };
			const float        v2 = a_speed * a_speed;
			const float        g = std::max(a_gravity, 1e-4f);
			const float        disc = v2 * v2 - g * (g * flat * flat + 2.0f * d.z * v2);
			const float        angle = disc >= 0.0f && flat > 1e-4f ? std::atan((v2 - std::sqrt(disc)) / (g * flat)) : 0.78539816f;
			const float        up = std::sin(angle), across = std::cos(angle);
			return { dir.x * across * a_speed, dir.y * across * a_speed, up * a_speed };
		}

		void Pick(Gun a_gun)
		{
			if (a_gun == c.gun) {
				return;
			}
			c.gun = a_gun;
			// No throw kept through a switch (the sandbox: a throw wound up, switched away and back,
			// went at once).
			c.bombTrigger = false;
			c.cooking = false;
			SKSE::log::info("combat: the {} is out", a_gun == Gun::kShatterbomb ? "Shatterbomb" : "Burst Pistol");
		}

		// The Shatterbomb's trigger, run the way libarsenal's Weapon runs a cooked one: holding
		// fire winds the throw up, letting go throws (the bomb leaves his hand a moment later).
		void Shatterbomb(float a_dt)
		{
			c.bombEmpty = c.bombAmmo == 0 ? c.bombEmpty + a_dt : 0.0f;
			if (c.bombEmpty > kBombRefill) {
				c.bombAmmo = kBombAmmo;
				c.bombEmpty = 0.0f;
			}
			// Busy (dashing, swinging, on a wall, the wrench out of his hand), a throw being wound
			// up waits: it would go on the release otherwise (the sandbox's).
			const bool busy = !CanFire(c.simState) || c.thrown || c.releaseIn >= 0.0f;
			if (!busy) {
				const bool released = !c.fire && c.bombTrigger;
				c.bombTrigger = c.fire;
				const bool wasReady = c.bombCooldown <= kReady;
				c.bombCooldown -= a_dt;
				if (released && c.bombCooldown <= kReady && c.bombAmmo >= 1 && c.bombIn < 0.0f) {
					--c.bombAmmo;
					// A throw due part-way through the step passes the rest on to the next wait.
					c.bombCooldown = (wasReady ? 0.0f : c.bombCooldown) + 1.0f / kBombRate;
					c.bombIn = kBombRelease;
					++c.bombThrows;
				} else {
					c.bombCooldown = std::max(c.bombCooldown, 0.0f);
				}
			}
			c.cooking = c.bombTrigger && c.bombAmmo >= 1 && c.bombCooldown <= kReady;
		}

		// The bomb leaves his hand, lobbed to come down where the crosshair is (aimed no further
		// than kBombRange).
		void ThrowBomb(RE::PlayerCharacter* a_player)
		{
			const float        s = Settings::Get().scale;
			const float        u = Units();
			const RE::NiPoint3 hand = HandAt(a_player);
			RE::NiPoint3       target = AimPoint().first;
			RE::NiPoint3       to = target - hand;
			if (to.Length() > kBombRange * s * u) {
				to.Unitize();
				target = hand + to * (kBombRange * s * u);
			}
			const RE::NiPoint3 v = LobVelocity(hand / u, target / u, kBombSpeed * s, kBombGravity * s);
			c.bombs.push_back({ hand, v });
			c.sinceShot = 0.0f;
			++c.tally.bombs;
			Sounds::Play("grenade_throw", hand);
			AutoTest::OnMove("bomb_thrown");
			SKSE::log::info("combat: a Shatterbomb leaves his hand for a point {:.1f} m away ({:.1f} m/s, {:.0f}° up)", to.Length() / u,
				v.Length(), std::asin(std::clamp(v.z / std::max(v.Length(), 1e-4f), -1.0f, 1.0f)) * 57.29578f);
		}

		// A Shatterbomb goes off at a_at: everyone in the blast's reach takes its damage, full
		// close by and falling off further out (measured to the nearest point of their body at
		// its middle height), and a stagger. Ratchet himself isn't hurt (Rift Apart's own).
		void Blast(RE::PlayerCharacter* a_player, const RE::NiPoint3& a_at)
		{
			const float s = Settings::Get().scale;
			const float u = Units();
			int         hits = 0;
			float       total = 0.0f;
			ForEachTarget(a_player, [&](RE::Actor* a_target) {
				const RE::NiPoint3 body = NearestOfBody(a_target, a_at) + RE::NiPoint3{ 0.0f, 0.0f, a_target->GetHeight() * 0.5f };
				const float        d = body.GetDistance(a_at) / (u * s);  // metres at his size
				if (d > kBlastOuter) {
					return;
				}
				const float t = std::clamp((d - kBlastInner) / (kBlastOuter - kBlastInner), 0.0f, 1.0f);
				const float damage = (kBlastInnerDamage + (kBlastOuterDamage - kBlastInnerDamage) * t) * Settings::Get().damageMult;
				Spells::Hit(a_target, damage, d <= kBlastHardPush ? 1.0f : 0.5f);
				++hits;
				total += damage;
			});
			c.tally.blasted += hits;
			c.sinceBlast = 0.0f;
			c.tally.damage += total;
			Spells::Blast(a_at);
			Sounds::Play("grenade_explode", a_at);
			// The game's explosion shake reaches him in full within 30 m (MediumExplosion's radii).
			if (a_player->GetPosition().GetDistance(a_at) < 30.0f * s * u) {
				RE::ShakeCamera(0.5f, a_at, 0.4f);
			}
			AutoTest::OnMove("blast");
			SKSE::log::info("combat: a Shatterbomb goes off {:.1f} m from him, hitting {} ({:.1f} damage in all)",
				a_player->GetPosition().GetDistance(a_at) / u, hits, total);
		}

		// The bombs fly (exactly, under constant gravity: libarsenal's Projectile::step) and go off
		// on the first thing they touch (the ray meets bodies too), or at the end of their life.
		void FlyBombs(RE::PlayerCharacter* a_player, float a_dt)
		{
			const float s = Settings::Get().scale;
			const float u = Units();
			const float g = kBombGravity * s;
			for (auto it = c.bombs.begin(); it != c.bombs.end();) {
				Bomb&              b = *it;
				const RE::NiPoint3 from = b.at;
				b.at += (b.velocity * a_dt + RE::NiPoint3{ 0.0f, 0.0f, -0.5f * g * a_dt * a_dt }) * u;
				b.velocity.z -= g * a_dt;
				b.age += a_dt;
				b.spin = std::fmod(b.spin + kBombSpin * (3.14159265f / 180.0f) * a_dt, 2.0f * 3.14159265f);
				std::optional<RE::NiPoint3> blast;
				if (const auto hit = Physics::Ray(from, b.at)) {
					blast = hit->point;
				} else if (b.age >= kBombLife) {
					blast = b.at;
				}
				if (blast) {
					Blast(a_player, *blast);
					it = c.bombs.erase(it);
				} else {
					++it;
				}
			}
		}

		void Pistol(RE::PlayerCharacter* a_player, float a_dt)
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
			const auto [aim, something] = AimPoint();
			RE::NiPoint3 dir = aim - muzzle;
			const float  distance = dir.Length();
			if (distance < 1.0f) {
				return;
			}
			dir.Unitize();
			Spells::Shoot(muzzle, Scatter(dir, c.spread), kShotDamage * Settings::Get().damageMult);
			Sounds::Play("pistol_shot", muzzle);
			if (c.shotsInARow == 3) {
				AutoTest::OnMove("shot");  // his aim pose is in by then
			}
			if (something) {
				// The bolt arrives when Skyrim's bolt would (it flies at 2500 units/s).
				Sounds::Play("bolt_impact", aim, distance / 2500.0f);
			}
			c.sinceShot = 0.0f;
			++c.tally.shots;
			++c.shots;
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

		// The favorites hotkeys that pick his guns don't also equip Skyrim's favorites on foot (the
		// other hotkeys still do).
		struct HotkeysOnFoot
		{
			static bool CanProcess(RE::FavoritesHandler* a_this, RE::InputEvent* a_event)
			{
				if (Jump::OnFoot() && a_event && a_event->GetEventType() == RE::INPUT_EVENT_TYPE::kButton) {
					const std::string_view event = a_event->QUserEvent().c_str();
					const auto&            s = Settings::Get();
					if (!event.empty() && (event == s.pistolControl || event == s.shatterbombControl)) {
						return false;
					}
				}
				return _CanProcess(a_this, a_event);
			}
			static inline REL::Relocation<decltype(CanProcess)> _CanProcess;
		};
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
		if (s.pistolControl.starts_with("Hotkey") || s.shatterbombControl.starts_with("Hotkey")) {
			REL::Relocation<std::uintptr_t> vtbl{ RE::VTABLE_FavoritesHandler[0] };
			HotkeysOnFoot::_CanProcess = vtbl.write_vfunc(0x1, HotkeysOnFoot::CanProcess);
			off += off.empty() ? " favorites on those hotkeys" : ", favorites on those hotkeys";
		}
		SKSE::log::info("combat: fire on {}, aim on {}, wrench on {}, throw on {}, the Burst Pistol on {}, the Shatterbomb on {}; Skyrim's own{} are off on foot",
			s.fireControl, s.aimControl, s.meleeControl, s.throwControl, s.pistolControl, s.shatterbombControl, off);
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
		// Picking a gun (not while the wrench is out of his hand: the game waits for the catch).
		const bool picks[2] = { Input::ControlHeld(s.pistolControl), Input::ControlHeld(s.shatterbombControl) };
		for (int i = 0; i < 2; ++i) {
			if (picks[i] && !c.pickHeld[i] && !c.thrown) {
				Pick(i == 0 ? Gun::kBurstPistol : Gun::kShatterbomb);
			}
			c.pickHeld[i] = picks[i];
		}
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
		if (c.gun == Gun::kBurstPistol) {
			Pistol(a_player, a_dt);
		} else {
			Shatterbomb(a_dt);
		}
		if (c.bombIn >= 0.0f) {
			c.bombIn -= a_dt;
			if (c.bombIn < 0.0f) {
				ThrowBomb(a_player);
			}
		}
		FlyBombs(a_player, a_dt);
		// A moment after a blast, its flash (for the tests' pictures).
		if (c.sinceBlast < 0.15f && c.sinceBlast + a_dt >= 0.15f) {
			AutoTest::OnMove("blast_flash");
		}
		c.sinceBlast += a_dt;
		c.aimAt = Strafing() ? std::optional(AimPoint().first) : std::nullopt;
	}

	void Reset()
	{
		c.bombs.clear();
		c.bombIn = -1.0f;
		c.bombTrigger = false;
		c.cooking = false;
	}

	Gun Selected() { return c.gun; }

	void Select(Gun a_gun) { Pick(a_gun); }

	bool WindingUp() { return c.gun == Gun::kShatterbomb && c.cooking && CanFire(c.simState); }

	std::uint32_t BombThrows() { return c.bombThrows; }

	std::vector<std::pair<RE::NiPoint3, float>> Bombs()
	{
		std::vector<std::pair<RE::NiPoint3, float>> out;
		out.reserve(c.bombs.size());
		for (const auto& b : c.bombs) {
			out.emplace_back(b.at, b.spin);
		}
		return out;
	}

	std::optional<RE::NiPoint3> AimTarget() { return c.aimAt; }

	std::uint32_t Shots() { return c.shots; }

	std::uint32_t Throws() { return c.throws; }

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
		if (c.gun == Gun::kBurstPistol && (c.aim || c.sinceShot < 2.0f)) {
			return Hand::kPistol;
		}
		return Hand::kNone;  // (the Shatterbomb's glove is on his hand while it's out)
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

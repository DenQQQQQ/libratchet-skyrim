// SPDX-License-Identifier: GPL-3.0-or-later
#include "Combat.h"

#include "AutoTest.h"
#include "Camera.h"
#include "Fx.h"
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
		// The crosshair's ray reaches further than the bolts fly (weapons.rs AIM_RANGE): a shot at
		// something past 45 m still goes its way.
		constexpr float kAimRange = 80.0f;
		// Its rounds (its level-1 kAmmo, one a shot): empty for a moment, both guns fill again.
		constexpr int kPistolAmmo = 150;
		// Its bolt (ShotPrius InitialMotion): 80 m/s, no gravity. With Rift Apart's effect art the
		// plugin flies it, as the sandbox does; without, Skyrim's spell bolt does.
		constexpr float kBoltSpeed = 80.0f;
		// Its bolt and the Shatterbomb are 0.25 m wide (libarsenal's projectile radius, × scale): a
		// body that near their path is hit (the sandbox's sweep).
		constexpr float kProjectileRadius = 0.25f;
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
			int   freeShots;                // shots in a row before it grows (MinShotsToIncrease)
		};
		constexpr Spread kHipSpread{ 0.1f, 0.05f, 4.0f, 25.0f, 3 };
		// (The aimed spread has no MinShotsToIncrease: it grows from the first shot.)
		constexpr Spread kAimedSpread{ 0.1f, 0.01f, 0.65f, 5.0f, 0 };

		// The Shatterbomb (wpn_protogrenade_weapon.json, as libarsenal reads it: its level-1
		// upgrades win). A cooked throw: holding fire winds it up, letting go throws, 0.8 a second,
		// 6 in the glove (filled again 1.5 s after the last, the sandbox's convenience). Lobbed at
		// 30 m/s under 30 m/s² to come down on the aimed point, aimed 30 m out at most, 8 s in the
		// air at most. Its blast: 170.5 damage within 1 m, falling to 85.25 at 6 m; knocked back
		// 3.5 m within 3 m, 1.5 m further out. Distances and speeds × scale. It leaves his hand at
		// the throw clip's AnimFireWeapon: 0.1 s into the whole-body throw standing, 0.0653 s into
		// the arm's on the move or in the air (the sandbox's choice of clip).
		constexpr float kBombRate = 0.8f;
		constexpr int   kBombAmmo = 6;
		constexpr float kBombRefill = 1.5f;
		constexpr float kBombSpeed = 30.0f;
		constexpr float kBombGravity = 30.0f;
		constexpr float kBombRange = 30.0f;
		constexpr float kBombLife = 8.0f;
		constexpr float kBombSpin = 500.0f;  // degrees/s, for its look
		constexpr float kBombRelease = 0.1f;
		constexpr float kBombReleaseArm = 0.0653f;
		constexpr float kStandingSpeed = 0.6f;  // m/s × scale: slower on the ground is standing
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
			std::string  sounds;    // its throw's sounds belong to it (its flight's hum stops at the blast)
			std::uint32_t id = 0;   // its throw's number (its glow's)
		};

		// A Burst Pistol bolt the plugin flies (game units).
		struct Bolt
		{
			RE::NiPoint3  at;
			RE::NiPoint3  dir;
			float         travelled = 0.0f;
			float         damage = 0.0f;
			std::uint32_t look = 0;  // its effect (Fx::Bolt)
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
			bool           faceThrow = false;   // he turns that way at the next step
			bool           meleeHeld = false;   // the melee button at the last step (pressed while aiming, it throws)
			bool           meleeLatched = false;  // a press held through a block (aiming, the wrench out) swings no more
			RatchetWrench* thrown = nullptr;    // in the air
			RE::NiPoint3   wrenchAt;            // where it is, game units
			float          wrenchSpin = 0.0f;
			std::uint8_t   blocked = 0;         // it hit a wall since its last step
			bool           turned = false;      // a wall turned it back (logged once)
			RE::NiPoint3   thrownFrom;
			float          thrownFor = 0.0f;    // seconds out
			float          thrownFar = 0.0f;    // metres out at most
			std::vector<RE::ActorHandle> struck;  // hit by this throw already
			float          throwAnim = 0.0f;    // seconds left of his throw's clip (the sandbox's throw_anim)
			std::uint32_t  catches = 0;
			std::uint32_t  equips = 0;          // weapons drawn so far
			std::optional<Gun> requested;       // a pick waiting for a Shatterbomb to leave his hand
			// The guns: which is out, and the Shatterbomb's trigger as libarsenal's Weapon keeps it.
			Gun               gun = Gun::kBurstPistol;
			bool              pickHeld[2] = {};
			bool              bombTrigger = false;  // the trigger at its last step (the throw goes on its release)
			float             bombCooldown = 0.0f;
			int               bombAmmo = kBombAmmo;
			int               pistolAmmo = kPistolAmmo;
			float             bombEmpty = 0.0f;     // seconds the gun out has been empty
			float             bombIn = -1.0f;       // a throw: seconds until the bomb leaves his hand
			bool              cooking = false;
			std::vector<Bomb> bombs;
			std::uint32_t     bombThrows = 0;
			std::uint32_t     bombsMade = 0;  // for each bomb's sounds' name
			// Shots on their way to someone: the assault they are, when the bolt gets there.
			std::vector<std::pair<RE::ActorHandle, float>> shotAt;
			// The pistol's own bolts in flight.
			std::vector<Bolt> bolts;
			float             arcFor = 0.0f;  // seconds the throw arc has shown
			float             sinceBlast = 100.0f;
			// Where he was at the last step, and in which world (a worldspace, or an interior cell).
			std::optional<RE::NiPoint3> lastAt;
			const void*                 lastWorld = nullptr;
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

		// No wrench throw while he swings (the combo, the Hyper-Strike) or is on a wall, a ledge or
		// a swing (the sandbox's start_throw); a dash doesn't stop one.
		bool CanThrow(std::uint32_t a_state)
		{
			const bool swinging = a_state >= RATCHET_STATE_MELEE && a_state <= RATCHET_STATE_SMASH_LAND;
			const bool holstered = a_state >= RATCHET_STATE_WALL_STICK && a_state <= RATCHET_STATE_SWING_JUMP;
			return !swinging && !holstered;
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
				// Where it lands: his side of their body, half their height up. One hit sound a
				// swing, at the first (the sandbox's).
				const RE::NiPoint3 at = NearestOfBody(a_target, me) + RE::NiPoint3{ 0.0f, 0.0f, a_target->GetHeight() * 0.5f };
				if (hits++ == 0) {
					Sounds::Play("wrench_hit", at);
				}
				RE::NiPoint3 way{ d.x, d.y, 0.0f };
				const float  l = way.Length();
				way = l > 1.0f ? way / l : RE::NiPoint3{ ahead.x, ahead.y, 0.0f };
				Fx::WrenchHit(at, way, a_target->GetPositionZ());
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
			Fx::WrenchSlam(me, radius * u);
			SKSE::log::info("combat: slam hits {} ({:.1f} damage within {:.1f} m)", hits, damage * Settings::Get().damageMult, radius);
		}

		// Where the camera looks, as far as kAimRange × scale: the point the shot goes for, and
		// whether something is there.
		std::pair<RE::NiPoint3, bool> AimPoint()
		{
			if (c.testAim) {
				return { *c.testAim, true };
			}
			const float reach = kAimRange * Settings::Get().scale * Units();
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
			const float s = Settings::Get().scale;
			const float u = Units();
			// In first person his model isn't drawn or posed: low and to the right in front of the
			// camera (the throw arc started from his third-person hand, wherever it was left).
			if (const auto* camera = RE::PlayerCamera::GetSingleton(); camera && camera->IsInFirstPerson() && camera->cameraRoot) {
				const auto& w = camera->cameraRoot->world;
				return w.translate + w.rotate.GetVectorY() * (0.4f * s * u) + w.rotate.GetVectorX() * (0.2f * s * u) - w.rotate.GetVectorZ() * (0.25f * s * u);
			}
			if (const auto hand = Model::GunHand()) {
				return *hand;
			}
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
			Sounds::Play("wrench_throw", hand);
			SKSE::log::info("combat: the wrench leaves his hand ({})", c.thrown ? "flying" : "the sim didn't throw it");
		}

		// Everyone within reach of its path from a_from to a_to takes the throw's hit, once. True if
		// anyone did: it turns back (the sandbox's), with one hit sound at the first.
		bool Strike(RE::PlayerCharacter* a_player, const RE::NiPoint3& a_from, const RE::NiPoint3& a_to)
		{
			int hits = 0;
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
				if (hits++ == 0) {
					Sounds::Play("wrench_hit", middle);
				}
				RE::NiPoint3 way{ path.x, path.y, 0.0f };
				const float  l = way.Length();
				way = l > 1.0f ? way / l : RE::NiPoint3{ 1.0f, 0.0f, 0.0f };
				Fx::WrenchHit(middle, way, a_target->GetPositionZ());
				SKSE::log::info("combat: the thrown wrench hits {}", a_target->GetName());
			});
			return hits > 0;
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
				// A wall turns it back, and so does anyone it hits (the sandbox's: it knocks them
				// back and comes home).
				const auto hit = Physics::Ray(c.wrenchAt, now);
				const bool wall = hit && !(hit->ref && hit->ref->As<RE::Actor>());
				const bool struck = Strike(a_player, c.wrenchAt, now);
				if (wall || struck) {
					c.blocked = 1;
					if (!c.turned) {
						c.turned = true;
						SKSE::log::info("combat: the wrench hit {} {:.1f} m out and turns back", wall ? (hit->ref ? hit->ref->GetName() : "the world") : "someone",
							now.GetDistance(c.thrownFrom) / Units());
					}
				}
			}
			Fx::WrenchTrail(c.wrenchAt, now);
			c.wrenchAt = now;
			c.wrenchSpin = spin;
			if (caught) {
				ratchet_wrench_free(c.thrown);
				c.thrown = nullptr;
				c.struck.clear();
				Sounds::Stop("wrench_throw_loop");
				Sounds::Play("wrench_catch", now);
				// The sandbox's SHOW_AFTER: his throw's clip ends soon after the catch, the wrench in
				// his hand till then.
				c.throwAnim = std::min(c.throwAnim, 0.35f);
				c.sinceWrench = 0.0f;
				++c.catches;
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
			++c.equips;
			SKSE::log::info("combat: the {} is out", a_gun == Gun::kShatterbomb ? "Shatterbomb" : "Burst Pistol");
		}

		// The guns don't fire: he's swinging, dashing, on a wall, a ledge or a swing, or the
		// wrench's throw has his hand, until just after the catch (the sandbox's busy: weapon_away,
		// and the wrench's throwing).
		bool GunsBusy() { return !CanFire(c.simState) || c.thrown || c.releaseIn >= 0.0f || c.throwAnim > 0.0f; }

		// The Burst Pistol put away (the Shatterbomb out): its wait runs down and its spread
		// settles back (libarsenal's Weapon::stow, at the hip's rate).
		void StowPistol(float a_dt)
		{
			c.cooldown = std::max(0.0f, c.cooldown - a_dt);
			c.shotsInARow = 0;
			c.spread = std::max(kHipSpread.min, c.spread - kHipSpread.recover * a_dt);
		}

		// The Shatterbomb's trigger, run the way libarsenal's Weapon runs a cooked one: holding
		// fire winds the throw up, letting go throws (the bomb leaves his hand a moment later).
		// Put away (the pistol out) or busy, it only follows the button as its wait runs down
		// (Weapon::stow): a throw still held when he's free again goes on its release, and one let
		// go meanwhile throws nothing. a_standing: on the ground, (nearly) still.
		void Shatterbomb(float a_dt, bool a_out, bool a_standing)
		{
			if (!a_out || GunsBusy()) {
				c.bombTrigger = c.fire;
				c.bombCooldown = std::max(0.0f, c.bombCooldown - a_dt);
			} else {
				const bool released = !c.fire && c.bombTrigger;
				c.bombTrigger = c.fire;
				const bool wasReady = c.bombCooldown <= kReady;
				c.bombCooldown -= a_dt;
				if (released && c.bombCooldown <= kReady && c.bombAmmo >= 1 && c.bombIn < 0.0f) {
					--c.bombAmmo;
					// A throw due part-way through the step passes the rest on to the next wait.
					c.bombCooldown = (wasReady ? 0.0f : c.bombCooldown) + 1.0f / kBombRate;
					c.bombIn = a_standing ? kBombRelease : kBombReleaseArm;
					++c.bombThrows;
				} else {
					c.bombCooldown = std::max(c.bombCooldown, 0.0f);
				}
			}
			// Winding up (the arc, his wind-up): held with a throw ready, busy or not.
			c.cooking = a_out && c.bombTrigger && c.bombAmmo >= 1 && c.bombCooldown <= kReady;
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
				to = target - hand;
			}
			const RE::NiPoint3 v = LobVelocity(hand / u, target / u, kBombSpeed * s, kBombGravity * s);
			const std::string sounds = std::format("shatterbomb {}", ++c.bombsMade);
			c.bombs.push_back({ hand, v, 0.0f, 0.0f, sounds, c.bombsMade });
			c.sinceShot = 0.0f;
			++c.tally.bombs;
			// Its throw: the toss, and its flight's hum riding with it until the blast (the game's
			// explosion event stops them; the sandbox's sounds owned by the grenade).
			Sounds::Play("grenade_throw", hand, 0.0f, sounds);
			AutoTest::OnMove("bomb_thrown");
			SKSE::log::info("combat: a Shatterbomb leaves his hand for a point {:.1f} m away ({:.1f} m/s, {:.0f}° up)", to.Length() / u,
				v.Length(), std::asin(std::clamp(v.z / std::max(v.Length(), 1e-4f), -1.0f, 1.0f)) * 57.29578f);
		}

		// While a throw winds up: where the Shatterbomb would fly from his hand to the crosshair
		// (as ThrowBomb would throw it now), stopping where it meets something (the sandbox's
		// throw_arc: 0.03 s steps, up to 6 s).
		void DrawThrowArc(RE::PlayerCharacter* a_player)
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
			const float        g = kBombGravity * s;
			std::vector<RE::NiPoint3>  points{ hand };
			std::optional<RE::NiPoint3> landing;
			for (int i = 1; i < 200; ++i) {
				const float        t = static_cast<float>(i) * 0.03f;
				const RE::NiPoint3 at = hand + (v * t + RE::NiPoint3{ 0.0f, 0.0f, -0.5f * g * t * t }) * u;
				if (const auto hit = Physics::Ray(points.back(), at)) {
					points.push_back(hit->point);
					landing = hit->point;
					break;
				}
				points.push_back(at);
			}
			Fx::ThrowArc(points, landing, kBlastInner * s * u);
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
			if (Fx::Available()) {
				// The ground under it, if it's near (the sandbox's: within 1.5 m of 0.2 m above it).
				const auto below = Physics::GroundDistance(a_at + RE::NiPoint3{ 0.0f, 0.0f, 0.2f * s * u }, 1.5f * s);
				const auto ground = below ? std::optional(a_at.z + 0.2f * s * u - *below * u) : std::nullopt;
				Fx::Explosion(a_at, kBlastOuter * s * u, ground);
			} else {
				Spells::Blast(a_at);
			}
			Sounds::Play("grenade_explode", a_at);
			// The game's explosion shake (MediumExplosion: full within 30 m, gone at 50 m); Skyrim's
			// own in its camera's place.
			if (!Camera::Explosion(a_at) && a_player->GetPosition().GetDistance(a_at) < 30.0f * s * u) {
				RE::ShakeCamera(0.5f, a_at, 0.4f);
			}
			AutoTest::OnMove("blast");
			SKSE::log::info("combat: a Shatterbomb goes off {:.1f} m from him, hitting {} ({:.1f} damage in all)",
				a_player->GetPosition().GetDistance(a_at) / u, hits, total);
		}

		// The first body within a_radius (game units) of the path from a_from to a_to, sampled every
		// 0.1 m × scale along it (the sandbox's dummy_contains sweep): the actor and where on the
		// path. A body: its middle line (NearestOfBody), as wide as its bounds, from its feet to its
		// height.
		std::optional<std::pair<RE::Actor*, RE::NiPoint3>> BodyAlong(RE::PlayerCharacter* a_player, const RE::NiPoint3& a_from, const RE::NiPoint3& a_to, float a_radius)
		{
			const float        len = a_from.GetDistance(a_to);
			const RE::NiPoint3 mid = (a_from + a_to) * 0.5f;
			std::vector<RE::Actor*> nearby;
			ForEachTarget(a_player, [&](RE::Actor* a_actor) {
				// Only those the path can reach (a dragon's long: its bounds count).
				const float extent = std::max(a_actor->GetBoundMax().GetDistance(a_actor->GetBoundMin()) * a_actor->GetScale(), 1.0f);
				if (a_actor->GetPosition().GetDistance(mid) <= len * 0.5f + extent + a_radius) {
					nearby.push_back(a_actor);
				}
			});
			if (nearby.empty()) {
				return std::nullopt;
			}
			const int steps = std::max(1, static_cast<int>(std::ceil(len / (0.1f * Settings::Get().scale * Units()))));
			for (int i = 0; i <= steps; ++i) {
				const RE::NiPoint3 p = a_from + (a_to - a_from) * (static_cast<float>(i) / static_cast<float>(steps));
				for (auto* actor : nearby) {
					const RE::NiPoint3 lo = actor->GetBoundMin(), hi = actor->GetBoundMax();
					const float        width = (hi.x - lo.x) * 0.5f * actor->GetScale();
					const float        feet = actor->GetPositionZ();
					if (p.z < feet - a_radius || p.z > feet + actor->GetHeight() + a_radius) {
						continue;
					}
					const RE::NiPoint3 axis = NearestOfBody(actor, p);
					const float        dx = p.x - axis.x, dy = p.y - axis.y;
					if (std::sqrt(dx * dx + dy * dy) <= width + a_radius) {
						return std::pair{ actor, p };
					}
				}
			}
			return std::nullopt;
		}

		// The pistol's bolts fly straight on at 80 m/s × scale to 45 m × scale, each step a ray
		// (libarsenal's Projectile::step and the sandbox's sweep): the first thing it meets stops it,
		// a body within its 0.25 m (× scale) too. A body takes its damage (the hit raises the
		// assault, as the wrench's do); level geometry takes a mark.
		void FlyBolts(RE::PlayerCharacter* a_player, float a_dt)
		{
			const float s = Settings::Get().scale;
			const float u = Units();
			const float range = kRange * s * u;
			for (std::size_t i = 0; i < c.bolts.size();) {
				auto&              b = c.bolts[i];
				const float        step = std::min(kBoltSpeed * s * u * a_dt, range - b.travelled);
				const RE::NiPoint3 next = b.at + b.dir * step;
				auto               hit = Physics::Ray(b.at, next);
				// A body near its path before what the ray met is what it hits.
				if (const auto body = BodyAlong(a_player, b.at, hit ? hit->point : next, kProjectileRadius * s * u)) {
					if (!hit || hit->ref != body->first) {
						SKSE::log::info("combat: a bolt grazes {} (within its 0.25 m)", body->first->GetName());
					}
					RE::NiPoint3 normal = body->second - NearestOfBody(body->first, body->second);
					normal.z = 0.0f;
					const float l = normal.Length();
					hit = Physics::Hit{ body->second, l > 1e-3f ? normal / l : b.dir * -1.0f, 0.0f, body->first };
				}
				bool done = false;
				if (hit) {
					auto* target = hit->ref ? hit->ref->As<RE::Actor>() : nullptr;
					Fx::BoltTrail(b.at, hit->point);
					Fx::MoveBolt(b.look, hit->point);
					Fx::EndBolt(b.look);
					Fx::BoltImpact(hit->point, hit->normal, target == nullptr);
					Sounds::Play("bolt_impact", hit->point);
					if (target && target != a_player && !target->IsDead()) {
						Spells::Hit(target, b.damage, 0.0f);
						c.tally.damage += b.damage;
					}
					done = true;
				} else {
					Fx::BoltTrail(b.at, next);
					Fx::MoveBolt(b.look, next);
					b.at = next;
					b.travelled += step;
					if (b.travelled >= range - 1.0f) {
						Fx::EndBolt(b.look);
						done = true;
					}
				}
				if (done) {
					b = c.bolts.back();
					c.bolts.pop_back();
				} else {
					++i;
				}
			}
		}

		// The bombs fly (exactly, under constant gravity: libarsenal's Projectile::step) and go off
		// on the first thing they touch (the ray meets bodies too, and a body within 0.25 m × scale
		// of the path counts: the sandbox's sweep), or at the end of their life.
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
				const auto                  hit = Physics::Ray(from, b.at);
				if (const auto body = BodyAlong(a_player, from, hit ? hit->point : b.at, kProjectileRadius * s * u)) {
					blast = body->second;
				} else if (hit) {
					blast = hit->point;
				} else if (b.age >= kBombLife) {
					blast = b.at;
				}
				if (blast) {
					Sounds::StopOwned(b.sounds);
					Blast(a_player, *blast);
					it = c.bombs.erase(it);
				} else {
					Sounds::Follow(b.sounds, b.at);
					Fx::BombGlow(b.id, b.at);
					++it;
				}
			}
		}

		// A load door, fast travel or a coc carried him off (another world, or 30 m in one step):
		// what's in the air stays behind, its coordinates the old place's, and the effects with it.
		// The wrench is back in his hand.
		void DropIfCarriedOff(RE::PlayerCharacter* a_player)
		{
			const RE::NiPoint3 at = a_player->GetPosition();
			const void*        world = a_player->GetWorldspace() ? static_cast<const void*>(a_player->GetWorldspace()) :
			                                                        static_cast<const void*>(a_player->GetParentCell());
			if ((c.lastAt && c.lastAt->GetDistance(at) > 30.0f * Units()) || (c.lastWorld && world != c.lastWorld)) {
				if (!c.bombs.empty() || c.thrown || !c.bolts.empty() || Fx::Live() > 0) {
					SKSE::log::info("combat: he was carried off: {} Shatterbombs and {} bolts in the air, {} effects dropped{}", c.bombs.size(),
						c.bolts.size(), Fx::Live(), c.thrown ? ", the wrench back in his hand" : "");
				}
				for (const auto& b : c.bombs) {
					Sounds::StopOwned(b.sounds);
				}
				c.bombs.clear();
				c.bolts.clear();
				Fx::Clear();
				if (c.thrown) {
					ratchet_wrench_free(c.thrown);
					c.thrown = nullptr;
					c.struck.clear();
					Sounds::Stop("wrench_throw_loop");
				}
			}
			c.lastAt = at;
			c.lastWorld = world;
		}

		// The glove fills again 1.5 s after it's empty, whichever gun is out.
		// The sandbox's: the gun out, empty for a moment (REFILL_AFTER), refills both.
		void Refill(float a_dt)
		{
			const bool empty = c.gun == Gun::kBurstPistol ? c.pistolAmmo == 0 : c.bombAmmo == 0;
			c.bombEmpty = empty ? c.bombEmpty + a_dt : 0.0f;
			if (c.bombEmpty > kBombRefill) {
				c.pistolAmmo = kPistolAmmo;
				c.bombAmmo = kBombAmmo;
				c.bombEmpty = 0.0f;
			}
		}

		void Pistol(RE::PlayerCharacter* a_player, float a_dt)
		{
			const Spread& spread = c.aim ? kAimedSpread : kHipSpread;
			// libarsenal's Weapon::step: a shot that came due part-way through the frame passes the
			// rest of it on to the next wait (the rate doesn't hang on the frame rate); one fired
			// from ready starts its wait now; a wait doesn't bank up while it isn't firing.
			const bool wasReady = c.cooldown <= kReady;
			c.cooldown -= a_dt;
			// (Not while he's busy: swinging, dashing, on a wall, or the wrench's throw has his hand.)
			if (!c.fire || GunsBusy() || c.pistolAmmo < 1) {
				c.spread = std::max(spread.min, c.spread - spread.recover * a_dt);
				c.shotsInARow = 0;
				c.cooldown = std::max(c.cooldown, 0.0f);
				return;
			}
			if (c.cooldown > kReady) {
				return;
			}
			c.cooldown = (wasReady ? 0.0f : c.cooldown) + 1.0f / (c.aim ? kAimedRate : kHipRate);
			--c.pistolAmmo;
			if (++c.shotsInARow > spread.freeShots) {
				c.spread = std::min(spread.max, c.spread + spread.grow);
			}
			c.spread = std::clamp(c.spread, spread.min, spread.max);
			// From the gun's muzzle (its igLoc_bulletEmit), as the sandbox fires it, or his hand.
			const auto         emit = Model::PistolLocator("igLoc_bulletEmit");
			const RE::NiPoint3 muzzle = emit ? emit->translate : HandAt(a_player);
			const auto [aim, something] = AimPoint();
			RE::NiPoint3 dir = aim - muzzle;
			const float  distance = dir.Length();
			if (distance < 1.0f) {
				return;
			}
			dir.Unitize();
			const RE::NiPoint3 shot = Scatter(dir, c.spread);
			const float        damage = kShotDamage * Settings::Get().damageMult;
			if (Fx::Available()) {
				// Rift Apart's bolt: the plugin flies it (FlyBolts), with the game's flash, bolt and
				// streak, and a spent shell out of the ejector.
				Fx::MuzzleFlash(muzzle, shot);
				c.bolts.push_back({ muzzle, shot, 0.0f, damage, Fx::Bolt(muzzle, shot) });
				if (const auto eject = Model::PistolLocator("igLoc_shell_eject")) {
					const auto ground = Physics::GroundDistance(eject->translate, 3.0f);
					Fx::Shell(eject->translate, eject->rotate, ground ? std::optional(eject->translate.z - *ground * Units()) : std::nullopt);
				}
			} else {
				Spells::Shoot(muzzle, shot, damage);
				// Who it's going to hit: Skyrim's bolt hurts them but raises nothing (the owner:
				// the pistol made nobody hostile), so the assault is ours, when the bolt gets there.
				if (const auto hit = Physics::Ray(muzzle, muzzle + shot * (kRange * Settings::Get().scale * Units()))) {
					if (auto* target = hit->ref ? hit->ref->As<RE::Actor>() : nullptr; target && target != a_player && !target->IsDead()) {
						c.shotAt.emplace_back(target->GetHandle(), hit->point.GetDistance(muzzle) / 2500.0f);
					}
				}
				if (something && distance <= kRange * Settings::Get().scale * Units()) {
					// The bolt arrives when Skyrim's bolt would (it flies at 2500 units/s).
					Sounds::Play("bolt_impact", aim, distance / 2500.0f);
				}
			}
			Sounds::Play("pistol_shot", muzzle);
			Camera::Recoil();
			if (c.shotsInARow == 3) {
				AutoTest::OnMove("shot");  // his aim pose is in by then
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
		// Picking a gun. Not while the wrench is out of his hand, from the throw until just after
		// the catch: the press does nothing (the sandbox's, as in the game). A Shatterbomb already
		// leaving his hand goes first, the switch right after it.
		const bool picks[2] = { Input::ControlHeld(s.pistolControl), Input::ControlHeld(s.shatterbombControl) };
		const bool wrenchOut = c.thrown != nullptr || c.releaseIn >= 0.0f || c.throwAnim > 0.0f;
		for (int i = 0; i < 2; ++i) {
			if (picks[i] && !c.pickHeld[i] && !wrenchOut) {
				c.requested = i == 0 ? Gun::kBurstPistol : Gun::kShatterbomb;
			}
			c.pickHeld[i] = picks[i];
		}
		if (c.requested && c.bombIn < 0.0f) {
			Pick(*c.requested);
			c.requested.reset();
		}
		// The throw: on its press, or melee pressed while aiming (Rift Apart's, and the sandbox's
		// throw_button), unless the wrench is out already or he's busy (swinging, on a wall or
		// ledge). His throw clip and the wrench in his hand start at the press; it leaves his hand
		// at the clip's release.
		const bool throwHeld = t.throwWrench || Input::ControlHeld(s.throwControl);
		const bool aimedMelee = c.aim && c.melee && !c.meleeHeld;
		c.meleeHeld = c.melee;
		if (((throwHeld && !c.throwHeld) || aimedMelee) && !c.thrown && c.releaseIn < 0.0f && CanThrow(a_state)) {
			c.releaseIn = kThrowRelease;
			c.throwAnim = 1.4f;  // the sandbox's THROW_ANIM
			c.throwView = a_view;
			c.faceThrow = true;
			c.sinceWrench = 0.0f;
			++c.throws;
		}
		c.throwHeld = throwHeld;
		// No swings while aiming, or while the wrench is out of his hand; and a press held through
		// that doesn't swing once it's back, until it's let go (the sandbox's rule, as the game's).
		const bool blocked = c.aim || c.thrown || c.releaseIn >= 0.0f;
		if (c.melee && (blocked || c.meleeLatched)) {
			c.meleeLatched = true;
			a_in.melee = 0;
		} else {
			c.meleeLatched = false;
			a_in.melee = c.melee ? 1 : 0;
		}
		// Aiming faces the aim and strafes; so does the trigger held (in a dash or a swing too: he
		// keeps facing the camera) and the moment after the newest shot, unless the wrench's throw
		// has his hand (a gun put away doesn't fire). The sandbox's.
		const bool throwing = c.thrown || c.releaseIn >= 0.0f || c.throwAnim > 0.0f;
		if (c.aim) {
			a_in.aiming = 1;
		} else if (!throwing && (c.fire || c.sinceShot < kStrafeAfterShot)) {
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
		c.throwAnim = std::max(0.0f, c.throwAnim - a_dt);
		if (c.faceThrow) {
			// The throw turns him along it at once (the sandbox's start_throw); the sim's yaw is
			// atan2(x, z) with z south.
			c.faceThrow = false;
			ratchet_set_facing(a_sim, std::atan2(c.throwView.x, -c.throwView.y));
		}
		DropIfCarriedOff(a_player);
		Refill(a_dt);
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
		// The gun put away follows the button too (the sandbox's stow).
		const float s = Settings::Get().scale;
		const bool  standing = a_r.state == RATCHET_STATE_GROUND && std::hypot(a_r.velocity.x, a_r.velocity.z) <= kStandingSpeed * s;
		if (c.gun == Gun::kBurstPistol) {
			Pistol(a_player, a_dt);
			Shatterbomb(a_dt, false, standing);
		} else {
			StowPistol(a_dt);
			Shatterbomb(a_dt, true, standing);
			if (WindingUp()) {
				DrawThrowArc(a_player);
				if (c.arcFor < 0.25f && c.arcFor + a_dt >= 0.25f) {
					AutoTest::OnMove("throw_arc");
				}
				c.arcFor += a_dt;
			} else {
				c.arcFor = 0.0f;
			}
		}
		FlyBolts(a_player, a_dt);
		for (auto& [target, in] : c.shotAt) {
			in -= a_dt;
			if (in <= 0.0f) {
				if (const auto actor = target.get()) {
					Spells::Attacked(a_player, actor.get());
				}
			}
		}
		std::erase_if(c.shotAt, [](const auto& a_shot) { return a_shot.second <= 0.0f; });
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
		for (const auto& b : c.bombs) {
			Sounds::StopOwned(b.sounds);
		}
		c.bombs.clear();
		c.bolts.clear();
		Fx::Clear();
		c.bombIn = -1.0f;
		c.bombTrigger = false;
		c.cooking = false;
	}

	void OffFoot(RE::PlayerCharacter* a_player, float a_dt)
	{
		// The buttons aren't his: a throw being wound up is dropped, not thrown the moment he's
		// back on foot, and nothing fires. What's in the air flies on.
		c.fire = false;
		c.aim = false;
		c.melee = false;
		c.bombTrigger = false;
		c.cooking = false;
		c.bombIn = -1.0f;
		c.releaseIn = -1.0f;
		if (!Settings::Get().combat || !a_player) {
			return;
		}
		DropIfCarriedOff(a_player);
		Refill(a_dt);
		if (c.thrown) {
			Fly(a_player, a_dt);
		}
		FlyBolts(a_player, a_dt);
		FlyBombs(a_player, a_dt);
	}

	Gun Selected() { return c.gun; }

	void Select(Gun a_gun) { Pick(a_gun); }

	bool WindingUp() { return c.gun == Gun::kShatterbomb && c.cooking; }

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

	AnimState ForAnimation()
	{
		AnimState a;
		a.shatterbomb = c.gun == Gun::kShatterbomb;
		a.aiming = c.aim;
		a.trigger = c.fire;
		a.stance = std::max(0.0f, kStrafeAfterShot - c.sinceShot);
		a.shots = c.shots;
		a.grenadeThrows = c.bombThrows;
		a.cooking = WindingUp();
		a.equips = c.equips;
		a.aimAt = c.aimAt ? *c.aimAt : AimPoint().first;
		a.wrenchOut = c.thrown != nullptr || c.releaseIn >= 0.0f || c.throwAnim > 0.0f;
		a.wrenchThrows = c.throws;
		a.wrenchCatches = c.catches;
		return a;
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
		// The sandbox's (weapons.rs gun_visibility and weapon_away, wrench.rs held_wrench): a dash
		// puts the weapon away for its length, a wall, a ledge or a swing until he's off it.
		const auto state = c.simState;
		const bool dashing = state == RATCHET_STATE_DASH || state == RATCHET_STATE_AIR_DASH;
		const bool holstered = state >= RATCHET_STATE_WALL_STICK && state <= RATCHET_STATE_SWING_JUMP;
		const bool swinging = state == RATCHET_STATE_MELEE || state == RATCHET_STATE_SMASH || state == RATCHET_STATE_SMASH_LAND;
		if (c.thrown || dashing || holstered) {
			return Hand::kNone;
		}
		// The wrench: while he swings and 0.35 s after (SHOW_AFTER), and on its way out of his hand.
		if (swinging || c.sinceWrench < 0.35f || c.releaseIn >= 0.0f) {
			return Hand::kWrench;
		}
		// The Burst Pistol: whenever it's the weapon, unless the wrench's throw has his hand (from
		// the throw until just after the catch).
		if (c.gun == Gun::kBurstPistol && c.throwAnim <= 0.0f) {
			return Hand::kPistol;
		}
		return Hand::kNone;  // (the Shatterbomb's glove is on his hand while it's out)
	}

	bool Strafing()
	{
		// As Fill hands it to the sim: aiming; or the trigger held or just after a shot, unless the
		// wrench's throw has his hand.
		const bool throwing = c.thrown || c.releaseIn >= 0.0f || c.throwAnim > 0.0f;
		return Settings::Get().combat && (c.aim || (!throwing && (c.fire || c.sinceShot < kStrafeAfterShot)));
	}

	void SetTestButtons(std::optional<TestButtons> a_buttons) { c.test = a_buttons; }

	void SetTestAim(std::optional<RE::NiPoint3> a_point) { c.testAim = a_point; }

	Tally TakeTally()
	{
		return std::exchange(c.tally, Tally{});
	}
}

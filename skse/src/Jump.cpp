// SPDX-License-Identifier: GPL-3.0-or-later
#include "Jump.h"

#include "AutoTest.h"
#include "Input.h"
#include "Model.h"
#include "RatchetDll.h"
#include "Settings.h"

namespace Jump
{
	namespace
	{
		// Skyrim game units per metre: Havok units are metres, and Skyrim's Havok scale is
		// 0.0142875 game units -> Havok units (about 70 units to the metre).
		float UnitsPerMetre() { return RE::bhkWorld::GetWorldScaleInverse(); }

		// libratchet is Y-up metres (right-handed, like Bevy); Skyrim is Z-up (x east, y north).
		// East stays +x, up is +y, and north is -z (Bevy's forward), which keeps the handedness,
		// so turns don't come out mirrored.
		RatchetVec3 ToSim(const RE::NiPoint3& a_units)
		{
			const float k = 1.0f / UnitsPerMetre();
			return { a_units.x * k, a_units.z * k, -a_units.y * k };
		}

		// One jump's numbers, for the log.
		struct Measure
		{
			float takeoffZ = 0.0f;  // game units
			float apexZ = 0.0f;
			float time = 0.0f;      // since takeoff
			float apexTime = 0.0f;
			float held = 0.0f;      // how long the button was held from the takeoff
			bool  released = false;
		};

		struct Host
		{
			RatchetSim*       sim = nullptr;
			bool              ours = false;           // in a jump we launched: we set his vertical speed
			bool              pressOnGround = false;  // this press began on the ground (presses in the air are masked: jump only)
			bool              prevHeld = false;
			RatchetStepResult last{};                 // the sim's last step
			float             lastDt = 0.0f;
			float             lastZ = 0.0f;           // his height when the sim last stepped, game units
			bool              falling = false;        // past the apex (JumpFall sent)
			float             savedGravity = 1.0f;    // the controller's own gravity, set to 0 while ours
			Measure           m;
		};
		Host g;

		bool Grounded(const RE::bhkCharacterController* a_ctrl)
		{
			return a_ctrl->context.currentState == RE::hkpCharacterStateType::kOnGround;
		}

		// Why the player isn't on foot and in control (Skyrim's own movement runs then and the sim
		// waits), or null if he is.
		const char* NotOnFoot(RE::PlayerCharacter* a_player, const RE::bhkCharacterController* a_ctrl)
		{
			const auto* state = a_player->AsActorState();
			if (!state) {
				return "no actor state";
			}
			if (a_player->IsDead()) {
				return "dead";
			}
			if (a_player->IsOnMount()) {
				return "mounted";
			}
			if (a_player->IsInKillMove()) {
				return "kill move";
			}
			if (a_player->IsInRagdollState() || state->GetKnockState() != RE::KNOCK_STATE_ENUM::kNormal) {
				return "ragdoll";
			}
			if (state->IsSwimming()) {
				return "swimming";
			}
			if (state->IsFlying()) {
				return "flying";
			}
			if (state->IsBleedingOut()) {
				return "bleeding out";
			}
			if (state->GetSitSleepState() != RE::SIT_SLEEP_STATE::kNormal) {
				return "furniture";
			}
			const auto cs = a_ctrl->context.currentState;
			if (cs != RE::hkpCharacterStateType::kOnGround && cs != RE::hkpCharacterStateType::kInAir && cs != RE::hkpCharacterStateType::kJumping) {
				return "controller state";
			}
			return nullptr;
		}

		bool OnFootInControl(RE::PlayerCharacter* a_player, const RE::bhkCharacterController* a_ctrl)
		{
			return NotOnFoot(a_player, a_ctrl) == nullptr;
		}

		bool Moving(const RE::bhkCharacterController* a_ctrl)
		{
			RE::hkVector4 v;
			a_ctrl->GetLinearVelocityImpl(v);
			const float x = v.quad.m128_f32[0], y = v.quad.m128_f32[1];
			return x * x + y * y > 1.0f;  // faster than 1 m/s across the ground
		}

		void Animate(RE::PlayerCharacter* a_player, const char* a_event)
		{
			if (Settings::Get().animations) {
				a_player->NotifyAnimationGraph(a_event);
			}
		}

		void EndOurs(RE::bhkCharacterController* a_ctrl)
		{
			if (g.ours && a_ctrl) {
				a_ctrl->gravity = g.savedGravity;
			}
			g.ours = false;
			g.falling = false;
		}

		void SetVerticalSpeed(RE::bhkCharacterController* a_ctrl, float a_metresPerSecond)
		{
			RE::hkVector4 v;
			a_ctrl->GetLinearVelocityImpl(v);
			v.quad.m128_f32[2] = a_metresPerSecond;
			a_ctrl->SetLinearVelocityImpl(v);
		}

		// Fall damage counts from his highest point, as Skyrim's own jumps do. fallStartHeight is
		// a height in game units (Skyrim sets it from the controller's Havok position * 69.99).
		void TrackFallStart(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl)
		{
			const float z = a_player->GetPositionZ();
			if (z > a_ctrl->fallStartHeight) {
				a_ctrl->fallStartHeight = z;
				a_ctrl->fallTime = 0.0f;
			}
		}

		void Launch(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl, float a_vy)
		{
			const bool moving = Moving(a_ctrl);
			g.ours = true;
			g.falling = false;
			g.savedGravity = a_ctrl->gravity;
			// Skyrim's in-air state would add its own gravity to the sim's speed.
			a_ctrl->gravity = 0.0f;
			a_ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
			a_ctrl->flags.set(RE::CHARACTER_FLAGS::kJumping);
			SetVerticalSpeed(a_ctrl, a_vy);
			a_ctrl->fallStartHeight = a_player->GetPositionZ();
			a_ctrl->fallTime = 0.0f;
			Animate(a_player, moving ? "JumpDirectionalStart" : "JumpStandingStart");

			g.m = {};
			g.m.takeoffZ = a_player->GetPositionZ();
			g.m.apexZ = g.m.takeoffZ;
		}

		void Land(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl)
		{
			Animate(a_player, "JumpLand");
			EndOurs(a_ctrl);
		}

		void ReportLanding(float a_landZ)
		{
			const float k = 1.0f / UnitsPerMetre();
			const AutoTest::Jump jump{
				.apex = (g.m.apexZ - g.m.takeoffZ) * k,
				.apexTime = g.m.apexTime,
				.hold = g.m.released ? g.m.held : 1.0f,
				.landDz = (a_landZ - g.m.takeoffZ) * k,
				.airTime = g.m.time,
			};
			AutoTest::OnLanded(jump);
			if (Settings::Get().logJumps && !AutoTest::Active()) {
				// The sandbox's apex for this hold.
				const auto expected = Ratchet::MeasureFlatJump(1.0f / 60.0f, jump.hold);
				SKSE::log::info("jump: apex {:.3f} m at {:.3f} s (sandbox {:.3f} m at {:.3f} s for a {:.3f} s hold), landed {:+.3f} m after {:.3f} s",
					jump.apex, jump.apexTime, expected.fullApex, expected.fullTime, jump.hold, jump.landDz, jump.airTime);
			}
		}

		void Step(RE::PlayerCharacter* a_player, float a_dt)
		{
			auto* ctrl = a_player->GetCharController();
			if (!ctrl || !g.sim) {
				return;
			}
			if (const char* why = NotOnFoot(a_player, ctrl)) {
				if (g.ours) {
					SKSE::log::info("jump: handed back to Skyrim mid-air ({})", why);
					AutoTest::OnHandedBack(why);
				}
				EndOurs(ctrl);
				ratchet_reset(g.sim);
				g.prevHeld = false;
				return;
			}

			const float z = a_player->GetPositionZ();
			const bool grounded = Grounded(ctrl);
			const bool held = AutoTest::JumpButton(a_dt, grounded, g.ours) || Input::JumpHeld();
			if (held && !g.prevHeld) {
				g.pressOnGround = grounded && !g.ours;
			}
			g.prevHeld = held;

			// Jump only: the sim sees the press that started on the ground until the jump turns
			// down (so no double jump, and no glide from holding on).
			RatchetInput in{};
			in.jump = held && g.pressOnGround && !(g.ours && g.last.velocity.y <= 0.0f) ? 1 : 0;
			RatchetContact contact{};
			contact.grounded = grounded ? 1 : 0;
			if (g.ours) {
				// Going up, he rose less than a quarter of what the sim asked for: something's above.
				const float asked = g.last.velocity.y * g.lastDt;
				const float rose = (z - g.lastZ) / UnitsPerMetre();
				contact.hit_ceiling = !grounded && asked > 0.01f && rose < 0.25f * asked ? 1 : 0;
				// Skyrim stood him on something while the sim still had him rising (a ledge he
				// came up level with): that's a landing.
				if (grounded && g.m.time > 0.05f && g.last.velocity.y > 0.0f) {
					ratchet_set_velocity(g.sim, { g.last.velocity.x, 0.0f, g.last.velocity.z });
				}
			}

			const RatchetStepResult r = ratchet_step(g.sim, a_dt, in, contact, ToSim(a_player->GetPosition()));

			if (r.events & RATCHET_EVENT_JUMPED) {
				Launch(a_player, ctrl, r.velocity.y);
				AutoTest::OnTakeoff();
			} else if (g.ours) {
				if (r.events & RATCHET_EVENT_LANDED || r.state == RATCHET_STATE_GROUND) {
					ReportLanding(z);
					Land(a_player, ctrl);
				} else {
					SetVerticalSpeed(ctrl, r.velocity.y);
					TrackFallStart(a_player, ctrl);
					if (!g.falling && r.velocity.y <= 0.0f) {
						g.falling = true;
						Animate(a_player, "JumpFall");
						AutoTest::OnApex();
					}
				}
			}

			if (g.ours) {
				g.m.time += a_dt;
				if (!g.m.released) {
					if (in.jump) {
						g.m.held += a_dt;
					} else {
						g.m.released = true;
					}
				}
			}
			if (Settings::Get().traceJumps && (g.ours || in.jump || r.events)) {
				SKSE::log::info("trace: dt {:.4f} held {} grounded {} ceiling {} state {} events {:#x} vy {:.3f} z {:.1f} ctrl {} fallStart {:.1f}",
					a_dt, in.jump, contact.grounded, contact.hit_ceiling, r.state, r.events, r.velocity.y, z,
					static_cast<int>(ctrl->context.currentState), ctrl->fallStartHeight);
			}
			g.last = r;
			g.lastDt = a_dt;
			g.lastZ = z;
		}

		// After Skyrim moved him this frame: the apex so far.
		void Observe(RE::PlayerCharacter* a_player)
		{
			if (!g.ours) {
				return;
			}
			const float z = a_player->GetPositionZ();
			if (z > g.m.apexZ) {
				g.m.apexZ = z;
				g.m.apexTime = g.m.time;
			}
		}

		// --- hooks

		// Skyrim's jump button: switched off on foot (the sim jumps instead). On a horse it stays
		// the horse's.
		struct JumpHandlerHook
		{
			static bool CanProcess(RE::JumpHandler* a_this, RE::InputEvent* a_event)
			{
				auto* player = RE::PlayerCharacter::GetSingleton();
				const auto* ctrl = player ? player->GetCharController() : nullptr;
				if (g.sim && ctrl && OnFootInControl(player, ctrl)) {
					return false;
				}
				return _CanProcess(a_this, a_event);
			}
			static inline REL::Relocation<decltype(CanProcess)> _CanProcess;
		};

		// Once a frame, before Skyrim moves the player: the sim steps and sets his vertical speed,
		// which Skyrim's controller then follows (checked in the traces: it does, frame by frame).
		struct PlayerUpdateHook
		{
			static void Update(RE::PlayerCharacter* a_this, float a_delta)
			{
				const float dt = std::clamp(a_delta, 0.0f, 0.1f);
				const bool running = dt > 0.0f && !RE::UI::GetSingleton()->GameIsPaused();
				if (running) {
					Step(a_this, dt);
				}
				_Update(a_this, a_delta);
				Observe(a_this);
				if (running) {
					// Ratchet's pose follows the move Skyrim has just made.
					const auto* ctrl = a_this->GetCharController();
					Model::Update(a_this, dt, { .simJump = g.ours, .simState = g.last.state, .events = g.last.events,
												  .grounded = !ctrl || Grounded(ctrl) });
				}
			}
			static inline REL::Relocation<decltype(Update)> _Update;
		};

	}

	void Install()
	{
		g.sim = Ratchet::NewSim();
		Input::LogBindings();

		REL::Relocation<std::uintptr_t> jumpVtbl{ RE::VTABLE_JumpHandler[0] };
		JumpHandlerHook::_CanProcess = jumpVtbl.write_vfunc(0x1, JumpHandlerHook::CanProcess);

		REL::Relocation<std::uintptr_t> playerVtbl{ RE::VTABLE_PlayerCharacter[0] };
		PlayerUpdateHook::_Update = playerVtbl.write_vfunc(0xAD, PlayerUpdateHook::Update);

		SKSE::log::info("jump: Skyrim's jump is off, libratchet's is on");
	}

	void Reset()
	{
		if (g.sim) {
			ratchet_reset(g.sim);
		}
		const auto* player = RE::PlayerCharacter::GetSingleton();
		EndOurs(player ? player->GetCharController() : nullptr);
		g.prevHeld = false;
		g.pressOnGround = false;
		g.last = {};
	}
}

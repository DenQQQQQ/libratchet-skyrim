// SPDX-License-Identifier: GPL-3.0-or-later
#include "Jump.h"

#include "AutoTest.h"
#include "Input.h"
#include "Model.h"
#include "Physics.h"
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
			RE::NiPoint3 takeoff;   // game units
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
			float             savedAcrobatics = 1.0f; // its air control, 0 while ours
			Measure           m;
			std::vector<Ratchet::Tick> ticks;         // this jump's frames, for the sandbox replay
			RatchetVec3       startVelocity{};        // his velocity as the jump started (sim axes)
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
				a_ctrl->acrobatics = g.savedAcrobatics;
			}
			g.ours = false;
			g.falling = false;
		}

		// The controller's velocity (Havok units = metres, Skyrim's axes) in the sim's axes.
		RatchetVec3 ControllerVelocity(const RE::bhkCharacterController* a_ctrl)
		{
			RE::hkVector4 v;
			a_ctrl->GetLinearVelocityImpl(v);
			return { v.quad.m128_f32[0], v.quad.m128_f32[2], -v.quad.m128_f32[1] };
		}

		// The sim's velocity onto the controller.
		void SetVelocity(RE::bhkCharacterController* a_ctrl, const RatchetVec3& a_sim)
		{
			RE::hkVector4 v;
			a_ctrl->GetLinearVelocityImpl(v);
			v.quad.m128_f32[0] = a_sim.x;
			v.quad.m128_f32[1] = -a_sim.z;
			v.quad.m128_f32[2] = a_sim.y;
			a_ctrl->SetLinearVelocityImpl(v);
		}

		// Where the camera looks: forward (3D) and right (on the ground plane), Skyrim's axes.
		struct View
		{
			RE::NiPoint3 forward{ 0.0f, 1.0f, 0.0f };
			RE::NiPoint2 flatForward{ 0.0f, 1.0f };
			RE::NiPoint2 flatRight{ 1.0f, 0.0f };
		};

		View CameraView()
		{
			View view;
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!camera || !camera->cameraRoot) {
				return view;
			}
			view.forward = camera->cameraRoot->world.rotate.GetVectorY();
			const float len = std::sqrt(view.forward.x * view.forward.x + view.forward.y * view.forward.y);
			if (len > 1e-4f) {
				view.flatForward = { view.forward.x / len, view.forward.y / len };
				view.flatRight = { view.flatForward.y, -view.flatForward.x };
			}
			return view;
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

		void Launch(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl, const RatchetVec3& a_velocity)
		{
			const bool moving = Moving(a_ctrl);
			g.ours = true;
			g.falling = false;
			g.savedGravity = a_ctrl->gravity;
			g.savedAcrobatics = a_ctrl->acrobatics;
			// Skyrim's in-air state would add its own gravity to the sim's speed, and pull the
			// speed across the ground toward its own locomotion ("acrobatics" is that pull, 1 in
			// vanilla).
			a_ctrl->gravity = 0.0f;
			a_ctrl->acrobatics = 0.0f;
			a_ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
			a_ctrl->flags.set(RE::CHARACTER_FLAGS::kJumping);
			SetVelocity(a_ctrl, a_velocity);
			a_ctrl->fallStartHeight = a_player->GetPositionZ();
			a_ctrl->fallTime = 0.0f;
			Animate(a_player, moving ? "JumpDirectionalStart" : "JumpStandingStart");

			g.m = {};
			g.ticks.clear();
			g.m.takeoff = a_player->GetPosition();
			g.m.takeoffZ = a_player->GetPositionZ();
			g.m.apexZ = g.m.takeoffZ;
		}

		void Land(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl)
		{
			Animate(a_player, "JumpLand");
			EndOurs(a_ctrl);
		}

		void ReportLanding(const RE::NiPoint3& a_land)
		{
			const float k = 1.0f / UnitsPerMetre();
			// The sandbox's jump for exactly these frames (time steps, button, stick) on a flat floor,
			// landing as high as he did.
			const float landDz = (a_land.z - g.m.takeoffZ) * k;
			const auto sandbox = Ratchet::ReplayFlat(g.ticks, g.startVelocity, landDz);
			const float dx = a_land.x - g.m.takeoff.x, dy = a_land.y - g.m.takeoff.y;
			const AutoTest::Jump jump{
				.apex = (g.m.apexZ - g.m.takeoffZ) * k,
				.apexTime = g.m.apexTime,
				.hold = g.m.released ? g.m.held : 1.0f,
				.landDz = landDz,
				.airTime = g.m.time,
				.distance = std::sqrt(dx * dx + dy * dy) * k,
				.sandboxApex = sandbox.apex,
				.sandboxApexTime = sandbox.apexTime,
				.sandboxAirTime = sandbox.airTime,
				.sandboxDistance = sandbox.distance,
			};
			AutoTest::OnLanded(jump);
			if (Settings::Get().logJumps && !AutoTest::Active()) {
				SKSE::log::info(
					"jump: apex {:.3f} m at {:.3f} s, {:.2f} m across (sandbox {:.3f} m at {:.3f} s, {:.2f} m for these frames), held {:.3f} s, landed {:+.3f} m after {:.3f} s",
					jump.apex, jump.apexTime, jump.distance, sandbox.apex, sandbox.apexTime, sandbox.distance, jump.hold, jump.landDz, jump.airTime);
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
			// The stick, turned by the camera: Rift Apart's controls are camera-relative.
			const View view = CameraView();
			const RE::NiPoint2 stick = Input::MoveStick();
			const float wx = stick.x * view.flatRight.x + stick.y * view.flatForward.x;
			const float wy = stick.x * view.flatRight.y + stick.y * view.flatForward.y;
			in.move_x = wx;
			in.move_z = -wy;
			in.view_right_x = view.flatRight.x;
			in.view_right_z = -view.flatRight.y;
			in.view_forward_x = view.forward.x;
			in.view_forward_y = view.forward.z;
			in.view_forward_z = -view.forward.y;
			RatchetContact contact{};
			contact.grounded = grounded ? 1 : 0;
			// How far down the ground is: the sim's jumps, double jump, glide and falls ask.
			if (const auto ground = Physics::GroundDistance(a_player->GetPosition(), 20.0f)) {
				contact.has_ground_distance = 1;
				contact.ground_distance = std::max(0.0f, *ground);
			}
			if (!g.ours) {
				// On the ground Skyrim moves him: the sim starts from what he really does across the
				// ground, so a jump keeps his speed. Not up and down: standing, Skyrim's vertical
				// speed can sit a hair above zero, and the sim lands only when not rising (a fall
				// it entered after a teleport never ended).
				const RatchetVec3 v = ControllerVelocity(ctrl);
				ratchet_set_velocity(g.sim, { v.x, g.last.velocity.y, v.z });
			}
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
				g.startVelocity = ControllerVelocity(ctrl);
				Launch(a_player, ctrl, r.velocity);
				AutoTest::OnTakeoff();
			} else if (g.ours) {
				if (r.events & RATCHET_EVENT_LANDED || r.state == RATCHET_STATE_GROUND) {
					ReportLanding(a_player->GetPosition());
					Land(a_player, ctrl);
				} else {
					// The whole velocity is the sim's in the air: up and across.
					SetVelocity(ctrl, r.velocity);
					TrackFallStart(a_player, ctrl);
					if (!g.falling && r.velocity.y <= 0.0f) {
						g.falling = true;
						Animate(a_player, "JumpFall");
						AutoTest::OnApex();
					}
				}
			}

			if (g.ours) {
				g.ticks.push_back({ a_dt, in.jump != 0, in.move_x, in.move_z });
				g.m.time += a_dt;
				if (!g.m.released) {
					if (in.jump) {
						g.m.held += a_dt;
					} else {
						g.m.released = true;
					}
				}
			}
			if (Settings::Get().traceJumps && (g.ours || in.jump || r.events || AutoTest::Active())) {
				SKSE::log::info(
					"trace: dt {:.4f} held {} grounded {} ground {}{:.2f} ceiling {} stick {:.2f},{:.2f} -> state {} events {:#x} v {:.2f},{:.3f},{:.2f} z {:.1f} ctrl {}",
					a_dt, in.jump, contact.grounded, contact.has_ground_distance ? "" : "none ", contact.ground_distance, contact.hit_ceiling,
					in.move_x, in.move_z, r.state, r.events, r.velocity.x, r.velocity.y, r.velocity.z, z, static_cast<int>(ctrl->context.currentState));
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
												  .grounded = !ctrl || Grounded(ctrl), .hasFacing = true, .facingYaw = g.last.facing_yaw });
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

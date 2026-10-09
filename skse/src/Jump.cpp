// SPDX-License-Identifier: GPL-3.0-or-later
#include "Jump.h"

#include "AutoTest.h"
#include "Combat.h"
#include "Input.h"
#include "Model.h"
#include "Physics.h"
#include "RatchetDll.h"
#include "Settings.h"
#include "Sounds.h"

namespace Jump
{
	namespace
	{
		// Skyrim game units per metre: Havok units are metres, and Skyrim's Havok scale is
		// 0.0142875 game units -> Havok units (about 70 units to the metre).
		float UnitsPerMetre() { return RE::bhkWorld::GetWorldScaleInverse(); }

		// Dashing, ground this close under his feet still holds him (metres): he's pressed down
		// onto it, but a slope going down falls away a little each frame.
		constexpr float kDashGroundReach = 0.3f;
		// In a flight, Skyrim's controller standing him on something is a landing when there's
		// ground this close under his feet (metres: a capsule on a 50° slope is about 0.2 m above
		// it), or once it has stood him there this long (seconds: an edge he rests on).
		constexpr float kLandReach = 0.5f;
		constexpr float kEdgeLanding = 0.25f;

		// The sandbox's body at scale 1 (apps/sandbox/src/main.rs BODY_HEIGHT, BODY_HALF_WIDTH,
		// WALL_PROBE_REACH): the wall probe looks from his middle, 1 m out, with side probes
		// 0.25 m to either side (Rift Apart's, at 0.35 m half width).
		constexpr float kBodyHeight = 1.2f;
		constexpr float kWallReach = 1.0f;
		constexpr float kWallProbeSide = 0.25f;

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
			float apexFrame = 0.0f; // the frame's length when the apex was seen (it can only be seen a frame late)
			float held = 0.0f;      // how long the button was held from the takeoff
			bool  released = false;
			bool  fromGround = true;  // a jump from the ground (not a double jump or glide out of Skyrim's fall)
		};

		struct Host
		{
			RatchetSim*       sim = nullptr;
			bool              ours = false;           // the sim moves him (a flight or a ground dash): we set his velocity
			bool              groundMove = false;     // ours on the ground: a dash, a wrench swing, the slam's landing
			RE::NiPoint3      dashStart;              // where the ground dash started, game units
			float             dashTime = 0.0f;
			RatchetStepResult last{};                 // the sim's last step
			float             lastDt = 0.0f;
			float             lastZ = 0.0f;           // his height when the sim last stepped, game units
			bool              falling = false;        // past the apex (JumpFall sent)
			float             fallEventAge = 0.0f;    // since the last fall event to the behaviour graph
			bool              fallDirectional = false;  // the last one was JumpFallDirectional
			float             savedGravity = 1.0f;    // the controller's own gravity, set to 0 while ours
			float             savedAcrobatics = 1.0f; // its air control, 0 while ours
			Measure           m;
			std::vector<Ratchet::Tick> ticks;         // this jump's (or dash's) frames, for the sandbox replay
			RatchetVec3       startVelocity{};        // his velocity as it started (sim axes)
			float             edgeTime = 0.0f;        // in a flight, Skyrim's controller has stood him on an edge this long
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

		// The controller follows the sim's velocity from here. Its own gravity would add to the
		// sim's speed, and its air control ("acrobatics", 1 in vanilla) pulls the speed across
		// the ground toward Skyrim's locomotion: both off.
		void TakeControl(RE::bhkCharacterController* a_ctrl)
		{
			if (!g.ours) {
				g.savedGravity = a_ctrl->gravity;
				g.savedAcrobatics = a_ctrl->acrobatics;
			}
			a_ctrl->gravity = 0.0f;
			a_ctrl->acrobatics = 0.0f;
			g.ours = true;
		}

		void EndOurs(RE::bhkCharacterController* a_ctrl)
		{
			if (g.ours && a_ctrl) {
				a_ctrl->gravity = g.savedGravity;
				a_ctrl->acrobatics = g.savedAcrobatics;
			}
			g.ours = false;
			g.groundMove = false;
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

		// The behaviour graph's fall state: MT_JumpFall plays once and sends its own JumpLand at
		// 5.23 s, which would land the Dragonborn's graph mid-air on a long glide. Sending the
		// other fall event of the pair restarts it (the same one again does nothing).
		void Fall(RE::PlayerCharacter* a_player)
		{
			g.falling = true;
			g.fallEventAge = 0.0f;
			g.fallDirectional = false;
			Animate(a_player, "JumpFall");
		}

		void KeepFalling(RE::PlayerCharacter* a_player, float a_dt)
		{
			g.fallEventAge += a_dt;
			if (g.falling && g.fallEventAge > 4.0f) {
				g.fallEventAge = 0.0f;
				g.fallDirectional = !g.fallDirectional;
				Animate(a_player, g.fallDirectional ? "JumpFallDirectional" : "JumpFall");
			}
		}

		// The sim's flight starts: a jump from the ground, or a double jump, glide or air dash out
		// of a fall Skyrim started (off a ledge), or a ground dash going off one. a_event goes to
		// the behaviour graph.
		void Launch(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl, const RatchetVec3& a_velocity, bool a_fromGround, const char* a_event)
		{
			TakeControl(a_ctrl);
			g.groundMove = false;
			g.falling = false;
			a_ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
			a_ctrl->flags.set(RE::CHARACTER_FLAGS::kJumping);
			SetVelocity(a_ctrl, a_velocity);
			a_ctrl->fallStartHeight = a_player->GetPositionZ();
			a_ctrl->fallTime = 0.0f;
			if (a_event == std::string_view("JumpFall")) {
				Fall(a_player);
			} else {
				Animate(a_player, a_event);
			}

			g.m = {};
			g.m.fromGround = a_fromGround;
			g.ticks.clear();
			g.m.takeoff = a_player->GetPosition();
			g.m.takeoffZ = a_player->GetPositionZ();
			g.m.apexZ = g.m.takeoffZ;
		}

		const char* TakeoffEvent(const RE::bhkCharacterController* a_ctrl)
		{
			return Moving(a_ctrl) ? "JumpDirectionalStart" : "JumpStandingStart";
		}

		// Rift Apart's sounds for this step's moves (the game's own events for them).
		void MoveSounds(RE::PlayerCharacter* a_player, const RatchetStepResult& a_r, std::uint32_t a_before)
		{
			const RE::NiPoint3 at = a_player->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, 1.0f * UnitsPerMetre() };
			const auto e = a_r.events;
			if (e & (RATCHET_EVENT_JUMPED | RATCHET_EVENT_WALL_JUMPED | RATCHET_EVENT_LEDGE_JUMPED)) {
				Sounds::Play("jump", at);
				Sounds::Play("jump_swish", at);
				Sounds::Play("footstep_jump", at);
				Sounds::Play("voice_jump", at);
			}
			if (e & RATCHET_EVENT_DOUBLE_JUMPED) {
				Sounds::Play("double_jump", at);
			}
			if (e & RATCHET_EVENT_LANDED) {
				Sounds::Play("land", at);
				Sounds::Play("footstep_land", at);
				Sounds::Play("voice_land", at);
			}
			if (e & RATCHET_EVENT_DASHED) {
				Sounds::Play("phantom_dash", at);
			} else if (a_before == RATCHET_STATE_DASH && a_r.state != RATCHET_STATE_DASH) {
				Sounds::Play("phantom_dash_end", at);
			}
			if (e & RATCHET_EVENT_AIR_DASHED) {
				Sounds::Play("air_dash", at);
			}
			if (e & RATCHET_EVENT_GLIDE_STARTED) {
				Sounds::Play("heli_extend", at);
			}
			if (a_r.state == RATCHET_STATE_GLIDE) {
				Sounds::Loop("heli_loop", at);
			} else if (a_before == RATCHET_STATE_GLIDE) {
				Sounds::Stop("heli_loop");
				Sounds::Play("heli_retract", at);
			}
			if (e & RATCHET_EVENT_MELEE_SWING) {
				Sounds::Play("wrench_swing", at);
				Sounds::Play("swing_foley", at);
				Sounds::Play(a_r.melee_swing >= 2 ? "voice_melee_heavy" : "voice_melee", at);
			}
			if (e & RATCHET_EVENT_SMASH_STARTED) {
				Sounds::Play("voice_melee_heavy", at);
			}
			if (e & RATCHET_EVENT_SMASH_IMPACT) {
				Sounds::Play("wrench_slam", at);
				Sounds::Play("wrench_slam_base", at);
				Sounds::Play("wrench_slam_sweetener", at);
			}
		}

		// The tests' ledge case logs the sim's rays (from where he stood as it stepped).
		bool        g_traceRays = false;
		RatchetVec3 g_rayOrigin{};

		// The sim's rays into Skyrim's world (it catches ledges with them): sim axes, metres.
		std::uint8_t WorldRay(void*, RatchetVec3 a_from, RatchetVec3 a_to, RatchetVec3* a_point, RatchetVec3* a_normal)
		{
			const float u = UnitsPerMetre();
			const auto game = [u](const RatchetVec3& a_v) { return RE::NiPoint3{ a_v.x * u, -a_v.z * u, a_v.y * u }; };
			const auto hit = Physics::Ray(game(a_from), game(a_to));
			if (g_traceRays) {
				// Relative to where he stood when the sim last stepped (sim axes, metres).
				const RatchetVec3 at = g_rayOrigin;
				SKSE::log::info("ray: ({:.2f} {:.2f} {:.2f}) -> ({:.2f} {:.2f} {:.2f}): {}", a_from.x - at.x, a_from.y - at.y, a_from.z - at.z,
					a_to.x - at.x, a_to.y - at.y, a_to.z - at.z,
					hit ? std::format("hit at {:.0f}% ({:.2f} {:.2f} {:.2f}) normal ({:.2f} {:.2f} {:.2f})", hit->fraction * 100.0f, ToSim(hit->point).x - at.x,
							  ToSim(hit->point).y - at.y, ToSim(hit->point).z - at.z, hit->normal.x, hit->normal.z, -hit->normal.y) :
						  "nothing");
			}
			if (!hit) {
				return 0;
			}
			*a_point = ToSim(hit->point);
			*a_normal = { hit->normal.x, hit->normal.z, -hit->normal.y };
			return 1;
		}

		// His flight path from here (velocity in sim axes, pulled down at a_gravity) for up to 4 s
		// in 32 steps, as Rift Apart predicts it to end a wall jump: true if the first thing it
		// meets is a floor (the sandbox's level::landing_ahead, with a point for his body).
		bool LandingAhead(const RE::NiPoint3& a_feet, const RatchetVec3& a_velocity, float a_gravity)
		{
			const float units = UnitsPerMetre();
			constexpr int   kSteps = 32;
			constexpr float kDt = 4.0f / kSteps;
			RE::NiPoint3 p = a_feet + RE::NiPoint3{ 0.0f, 0.0f, 0.1f * units };
			RE::NiPoint3 v{ a_velocity.x, -a_velocity.z, a_velocity.y };  // Skyrim's axes, m/s
			for (int i = 0; i < kSteps; ++i) {
				const RE::NiPoint3 next = v - RE::NiPoint3{ 0.0f, 0.0f, a_gravity * kDt };
				const RE::NiPoint3 to = p + (v + next) * (0.5f * kDt * units);
				if (const auto hit = Physics::Ray(p, to)) {
					return hit->normal.z > 0.7f;
				}
				p = to;
				v = next;
			}
			return false;
		}

		// The sim moves him along the ground: a dash, a wrench swing's lunge, the slam's landing.
		// Skyrim's on-ground state moves him by its own locomotion, whatever velocity it's
		// given; its in-air state keeps the one it's given. So he's put in the air state every
		// frame, with the sim's velocity, which presses him down onto the ground at 2 m/s
		// (× scale): the ground holds him up.
		bool GroundMove(std::uint32_t a_state)
		{
			return a_state == RATCHET_STATE_DASH || a_state == RATCHET_STATE_MELEE || a_state == RATCHET_STATE_SMASH_LAND;
		}

		void StartGroundMove(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl, bool a_dash)
		{
			TakeControl(a_ctrl);
			g.groundMove = true;
			if (a_dash) {
				g.dashStart = a_player->GetPosition();
				g.dashTime = 0.0f;
				g.ticks.clear();
				g.startVelocity = ControllerVelocity(a_ctrl);
			}
		}

		void ReportDash(const RE::NiPoint3& a_end)
		{
			const float k = 1.0f / UnitsPerMetre();
			const float dx = a_end.x - g.dashStart.x, dy = a_end.y - g.dashStart.y;
			const AutoTest::Dash dash{
				.distance = std::sqrt(dx * dx + dy * dy) * k,
				.time = g.dashTime,
				.climb = (a_end.z - g.dashStart.z) * k,
				.sandbox = Ratchet::ReplayDash(g.ticks, g.startVelocity),
			};
			AutoTest::OnDashed(dash);
			if (Settings::Get().logJumps && !AutoTest::Active()) {
				SKSE::log::info("dash: {:.2f} m in {:.3f} s, {:+.2f} m up (sandbox {:.2f} m in {:.3f} s for these frames)",
					dash.distance, dash.time, dash.climb, dash.sandbox.distance, dash.sandbox.time);
			}
		}

		void Land(RE::PlayerCharacter* a_player, RE::bhkCharacterController* a_ctrl)
		{
			// The landing that goes with the fall state the graph is in.
			Animate(a_player, g.falling && g.fallDirectional ? "JumpLandDirectional" : "JumpLand");
			EndOurs(a_ctrl);
		}

		void ReportLanding(const RE::NiPoint3& a_land, float a_dt)
		{
			const float k = 1.0f / UnitsPerMetre();
			const float landDz = (a_land.z - g.m.takeoffZ) * k;
			const float dx = a_land.x - g.m.takeoff.x, dy = a_land.y - g.m.takeoff.y;
			const float distance = std::sqrt(dx * dx + dy * dy) * k;
			if (!g.m.fromGround) {
				// Out of Skyrim's fall, or a wall or ledge took part: a flat-floor replay can't
				// follow it, so there's none.
				AutoTest::OnLanded({ .apex = (g.m.apexZ - g.m.takeoffZ) * k, .apexTime = g.m.apexTime, .hold = g.m.released ? g.m.held : 1.0f,
					.landDz = landDz, .airTime = g.m.time, .distance = distance, .replayed = false });
				if (Settings::Get().logJumps && !AutoTest::Active()) {
					SKSE::log::info("jump: {:.2f} m across, landed {:+.3f} m after {:.3f} s (no replay: out of a fall, or a wall or ledge)", distance, landDz, g.m.time);
				}
				return;
			}
			// The sandbox's jump for exactly these frames (time steps, button, stick) on a flat floor,
			// landing as high as he did.
			const auto sandbox = Ratchet::ReplayFlat(g.ticks, g.startVelocity, landDz);
			const AutoTest::Jump jump{
				.apex = (g.m.apexZ - g.m.takeoffZ) * k,
				.apexTime = g.m.apexTime,
				.hold = g.m.released ? g.m.held : 1.0f,
				.landDz = landDz,
				.airTime = g.m.time,
				.distance = distance,
				.apexFrame = g.m.apexFrame,
				.landFrame = a_dt,
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
				return;
			}

			const float z = a_player->GetPositionZ();
			const RE::NiPoint3 feet = a_player->GetPosition();
			// How far down the ground is: the sim's jumps, double jump, glide and falls ask.
			const auto ground = Physics::GroundDistance(feet, 20.0f);
			// Skyrim's controller says whether he stands. In a ground move it's kept in its air
			// state, and then the ground right under his feet is what holds him up.
			bool grounded = Grounded(ctrl) || (g.groundMove && ground && *ground < kDashGroundReach);
			// In a flight, its capsule (the Dragonborn's, half again his height) also "stands" on
			// an edge it meets at his chest, with nothing under his feet: a ledge he'd catch. Such
			// a stand isn't a landing, unless it lasts (an edge he really rests on).
			if (g.ours && !g.groundMove && Grounded(ctrl) && !(ground && *ground < kLandReach)) {
				g.edgeTime += a_dt;
				if (g.edgeTime < kEdgeLanding) {
					grounded = false;
					ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
				}
			} else {
				g.edgeTime = 0.0f;
			}
			// The buttons as they are: the sim makes the jump, double jump, glide and dashes of them.
			const AutoTest::Buttons test = AutoTest::Press(a_dt, grounded, g.ours, g.last.state);
			RatchetInput in{};
			in.jump = test.jump || Input::JumpHeld() ? 1 : 0;
			in.dash = test.dash || Input::DashHeld() ? 1 : 0;
			// Crouch lets go of a ledge: Skyrim's Sneak.
			in.crouch = Input::SneakHeld() ? 1 : 0;
			// The stick, turned by the camera: Rift Apart's controls are camera-relative.
			const View view = CameraView();
			RE::NiPoint2 stick = Input::MoveStick();
			// Sneaking, he walks (Rift Apart has no sneak).
			if (a_player->IsSneaking()) {
				stick.x *= 0.5f;
				stick.y *= 0.5f;
			}
			const float wx = stick.x * view.flatRight.x + stick.y * view.flatForward.x;
			const float wy = stick.x * view.flatRight.y + stick.y * view.flatForward.y;
			in.move_x = wx;
			in.move_z = -wy;
			in.view_right_x = view.flatRight.x;
			in.view_right_z = -view.flatRight.y;
			// The wrench and aiming (the gun fires after the step).
			Combat::Fill(in, RE::NiPoint2{ view.flatForward.x, view.flatForward.y }, g.last.state, a_dt);
			in.view_forward_x = view.forward.x;
			in.view_forward_y = view.forward.z;
			in.view_forward_z = -view.forward.y;
			RatchetContact contact{};
			contact.grounded = grounded ? 1 : 0;
			if (ground) {
				contact.has_ground_distance = 1;
				contact.ground_distance = std::max(0.0f, *ground);
			}
			// The wall beside him: the sim sticks to walls in the air. Skyrim marks none for wall
			// jumps, so every wall is a plain one (one wall jump per jump) and there are no
			// wall-jump zones.
			if (!grounded || (g.ours && !g.groundMove)) {
				const float s = Settings::Get().scale;
				std::optional<RE::NiPoint2> prefer;
				if (g.last.state == RATCHET_STATE_WALL_STICK) {
					prefer = RE::NiPoint2{ g.last.wall_normal_x, -g.last.wall_normal_z };
				}
				if (const auto wall = Physics::WallProbe(feet, 0.5f * kBodyHeight * s, kWallReach * s, kWallProbeSide * s, prefer)) {
					contact.has_wall = 1;
					contact.wall_normal_x = wall->normal.x;
					contact.wall_normal_z = -wall->normal.y;
					contact.has_wall_distance = 1;
					contact.wall_distance = wall->distance;
				}
				if (g.last.state == RATCHET_STATE_WALL_JUMP) {
					contact.landing_ahead = LandingAhead(feet, g.last.velocity, g.last.gravity) ? 1 : 0;
				}
			}
			if (!g.ours || (g.groundMove && g.last.state == RATCHET_STATE_GROUND)) {
				// The sim goes on from what he really does across the ground: Skyrim's locomotion, or
				// the sim's own walk after the collisions (a wall stops him). So a jump keeps his
				// speed. Not up and down: standing, Skyrim's vertical speed can sit a hair above
				// zero, and the sim lands only when not rising (a fall it entered after a teleport
				// never ended).
				const RatchetVec3 v = ControllerVelocity(ctrl);
				ratchet_set_velocity(g.sim, { v.x, g.last.velocity.y, v.z });
			}
			if (g.ours && !g.groundMove) {
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

			g_traceRays = Settings::Get().traceJumps && g.ours && !g.groundMove && AutoTest::TraceRays();
			g_rayOrigin = ToSim(feet);
			const RatchetStepResult r = ratchet_step(g.sim, a_dt, in, contact, ToSim(feet));
			g_traceRays = false;
			const Ratchet::Tick tick{ a_dt, in.jump != 0, in.move_x, in.move_z, in.dash != 0, in.melee != 0 };

			// A flight ended (a new jump can start in the same step); the slam lands into its
			// ground move.
			if (g.ours && !g.groundMove &&
				(r.events & RATCHET_EVENT_LANDED || r.state == RATCHET_STATE_GROUND || r.state == RATCHET_STATE_SMASH_LAND)) {
				ReportLanding(feet, a_dt);
				Land(a_player, ctrl);
			}
			const bool wasGroundMove = g.groundMove;
			// The sim walks and runs him too (INI bSimGround) while he's moving: Skyrim's own
			// locomotion idles 0.14 s and takes 0.4 s to its run (measured), Rift Apart's run is
			// there in about 0.16 s. Standing still, Skyrim's ground state has him (waiting,
			// sitting, talking).
			const bool walking = Settings::Get().simGround && r.state == RATCHET_STATE_GROUND &&
			                     (stick.x * stick.x + stick.y * stick.y > 0.0025f || r.velocity.x * r.velocity.x + r.velocity.z * r.velocity.z > 0.05f);
			const bool groundMove = GroundMove(r.state) || walking;
			// The dash is over (into a swing, back on his feet, off a ledge, a jump).
			if (g.groundMove && g.last.state == RATCHET_STATE_DASH && r.state != RATCHET_STATE_DASH) {
				g.ticks.push_back(tick);
				ReportDash(feet);
			}
			// The ground move ended: standing still, off a ledge, or jumping out of it.
			if (g.groundMove && !groundMove) {
				if (r.state == RATCHET_STATE_GROUND) {
					// Skyrim's ground state takes over, at the speed the sim left him with.
					EndOurs(ctrl);
				} else if (!(r.events & RATCHET_EVENT_JUMPED)) {
					// Off a ledge: a fall at the speed he had.
					Launch(a_player, ctrl, r.velocity, false, "JumpFall");
				}
			}
			bool started = true;
			if (r.events & RATCHET_EVENT_JUMPED) {
				g.startVelocity = ControllerVelocity(ctrl);
				// Out of a dash or swing it keeps their speed, which a replay from standing can't.
				// (Out of the sim's own run it's the same as Skyrim's: the speed he really has.)
				Launch(a_player, ctrl, r.velocity, !wasGroundMove || g.last.state == RATCHET_STATE_GROUND, TakeoffEvent(ctrl));
				AutoTest::OnTakeoff();
			} else if (groundMove && (!g.groundMove || (r.events & RATCHET_EVENT_DASHED))) {
				// A dash or a swing from his feet (or a dash out of a swing), or the slam's landing.
				StartGroundMove(a_player, ctrl, r.state == RATCHET_STATE_DASH);
				if (r.state == RATCHET_STATE_DASH) {
					AutoTest::OnTakeoff();
					AutoTest::OnMove("dash");
				}
			} else if (!g.ours && r.state != RATCHET_STATE_GROUND &&
					   r.events & (RATCHET_EVENT_DOUBLE_JUMPED | RATCHET_EVENT_GLIDE_STARTED | RATCHET_EVENT_AIR_DASHED | RATCHET_EVENT_WALL_STUCK |
								   RATCHET_EVENT_LEDGE_GRABBED)) {
				// Out of a fall Skyrim started (off a ledge): the sim's from here.
				g.startVelocity = ControllerVelocity(ctrl);
				Launch(a_player, ctrl, r.velocity, false, r.events & RATCHET_EVENT_DOUBLE_JUMPED ? TakeoffEvent(ctrl) : "JumpFall");
			} else {
				started = false;
			}

			if (g.groundMove) {
				// In the air state with the sim's velocity (see StartGroundMove). Bumps along the way
				// aren't falls.
				ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
				RatchetVec3 v = r.velocity;
				// Down a slope or a step the ground falls away under him: he follows it while it's
				// in reach (Skyrim's ground state would snap him down too).
				if (ground && *ground > 0.02f && *ground < kDashGroundReach) {
					v.y = std::min(v.y, -*ground / a_dt);
				}
				SetVelocity(ctrl, v);
				ctrl->fallStartHeight = z;
				ctrl->fallTime = 0.0f;
			} else if (g.ours && !started) {
				if (r.events & RATCHET_EVENT_AIR_DASHED) {
					AutoTest::OnMove("air_dash");
				}
				if (r.events & RATCHET_EVENT_DOUBLE_JUMPED) {
					// The takeoff animation again, and falls count from this jump on.
					g.falling = false;
					Animate(a_player, TakeoffEvent(ctrl));
					ctrl->fallStartHeight = z;
					ctrl->fallTime = 0.0f;
					AutoTest::OnMove("double_jump");
				}
				if (r.events & RATCHET_EVENT_GLIDE_STARTED) {
					AutoTest::OnMove("glide");
				}
				if (r.events & (RATCHET_EVENT_WALL_STUCK | RATCHET_EVENT_LEDGE_GRABBED)) {
					// The level took part: a flat-floor replay can't follow this flight.
					g.m.fromGround = false;
					AutoTest::OnMove(r.events & RATCHET_EVENT_WALL_STUCK ? "wall_stick" : "ledge");
				}
				if (r.events & RATCHET_EVENT_LEDGE_CLIMBED) {
					AutoTest::OnMove("ledge_climb");
				}
				if (r.events & RATCHET_EVENT_WALL_JUMPED) {
					// A jump again, off the wall: falls count from it.
					g.falling = false;
					Animate(a_player, "JumpDirectionalStart");
					ctrl->fallStartHeight = z;
					ctrl->fallTime = 0.0f;
					AutoTest::OnMove("wall_jump");
				}
				// The whole velocity is the sim's in the air: up and across.
				SetVelocity(ctrl, r.velocity);
				if (r.state == RATCHET_STATE_GLIDE || r.state == RATCHET_STATE_WALL_STICK || r.state == RATCHET_STATE_LEDGE_HANG ||
					r.state == RATCHET_STATE_LEDGE_CLIMB || r.state == RATCHET_STATE_SMASH) {
					// A glide comes down softly, and so does a slide down a wall; hanging from a
					// ledge he's held; the Hyper-Strike's slam never hurts him. The height they
					// sink doesn't count as a fall.
					ctrl->fallStartHeight = z;
					ctrl->fallTime = 0.0f;
				}
				TrackFallStart(a_player, ctrl);
				if (!g.falling && r.velocity.y <= 0.0f) {
					Fall(a_player);
					AutoTest::OnMove("apex");
				}
				KeepFalling(a_player, a_dt);
			}

			if (g.groundMove) {
				if (r.state == RATCHET_STATE_DASH) {
					g.ticks.push_back(tick);
					g.dashTime += a_dt;
				}
			} else if (g.ours) {
				g.ticks.push_back(tick);
				g.m.time += a_dt;
				if (!g.m.released) {
					if (in.jump) {
						g.m.held += a_dt;
					} else {
						g.m.released = true;
					}
				}
			}
			if (r.events & RATCHET_EVENT_SMASH_STARTED) {
				AutoTest::OnMove("smash");
			}
			if (r.events & RATCHET_EVENT_MELEE_SWING) {
				AutoTest::OnMove(std::format("swing{}", r.melee_swing + 1));
			}
			MoveSounds(a_player, r, g.last.state);
			// The wrench's hits, the slam, the gun.
			Combat::Step(a_player, g.sim, r, a_dt);
			if (Settings::Get().traceJumps && (g.ours || in.jump || in.dash || r.events || AutoTest::Active())) {
				SKSE::log::info(
					"trace: dt {:.4f} jump {} dash {} grounded {} ground {}{:.2f} ceiling {} stick {:.2f},{:.2f} -> state {} events {:#x} v {:.2f},{:.3f},{:.2f} facing {:.2f} z {:.1f} ctrl {}",
					a_dt, in.jump, in.dash, contact.grounded, contact.has_ground_distance ? "" : "none ", contact.ground_distance, contact.hit_ceiling,
					in.move_x, in.move_z, r.state, r.events, r.velocity.x, r.velocity.y, r.velocity.z, r.facing_yaw, z,
					static_cast<int>(ctrl->context.currentState));
			}
			g.last = r;
			g.lastDt = a_dt;
			g.lastZ = z;
		}

		// After Skyrim moved him this frame: the apex so far.
		void Observe(RE::PlayerCharacter* a_player)
		{
			if (!g.ours || g.groundMove) {
				return;
			}
			const float z = a_player->GetPositionZ();
			if (z > g.m.apexZ) {
				g.m.apexZ = z;
				g.m.apexTime = g.m.time;
				g.m.apexFrame = g.lastDt;
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

		// Skyrim's sprint, when its button is the dash's: switched off on foot (Rift Apart has no
		// sprint). On a horse it stays the horse's gallop.
		struct SprintHandlerHook
		{
			static bool CanProcess(RE::SprintHandler* a_this, RE::InputEvent* a_event)
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

		bool DashOnSprint()
		{
			const auto& control = Settings::Get().dashControl;
			return control.size() == 6 && _strnicmp(control.c_str(), "Sprint", 6) == 0;
		}

		// Once a frame, before Skyrim moves the player: the sim steps and sets his velocity,
		// which Skyrim's controller then follows (checked in the traces: it does, frame by frame).
		struct PlayerUpdateHook
		{
			static void Update(RE::PlayerCharacter* a_this, float a_delta)
			{
				const float dt = std::clamp(a_delta, 0.0f, 0.1f);
				const bool running = dt > 0.0f && !RE::UI::GetSingleton()->GameIsPaused();
				if (running) {
					Step(a_this, dt);
					Sounds::Update(dt);
				}
				_Update(a_this, a_delta);
				Observe(a_this);
				if (running) {
					// Ratchet's pose follows the move Skyrim has just made.
					const auto* ctrl = a_this->GetCharController();
					// Strafing: the way he moves, from where he faces (the sim's facing, on Skyrim's
					// ground plane: ahead (sin f, -cos f), right (-cos f, -sin f)).
					float strafeAngle = 0.0f;
					if (ctrl) {
						RE::hkVector4 v;
						ctrl->GetLinearVelocityImpl(v);
						const float vx = v.quad.m128_f32[0], vy = v.quad.m128_f32[1];
						const float f = g.last.facing_yaw;
						strafeAngle = std::atan2(vx * -std::cos(f) + vy * -std::sin(f), vx * std::sin(f) + vy * -std::cos(f));
					}
					Model::Update(a_this, dt, { .simJump = g.ours, .simState = g.last.state, .events = g.last.events,
												  .grounded = !ctrl || Grounded(ctrl), .wallRight = g.last.wall_right != 0,
												  .ledgeMove = g.last.ledge_move, .verticalSpeed = g.last.velocity.y,
												  .meleeSwing = g.last.melee_swing, .strafing = Combat::Strafing(), .strafeAngle = strafeAngle,
												  .aimAt = Combat::AimTarget(), .shots = Combat::Shots(), .throws = Combat::Throws(),
												  .hasFacing = true, .facingYaw = g.last.facing_yaw });
				}
			}
			static inline REL::Relocation<decltype(Update)> _Update;
		};

	}

	void Install()
	{
		g.sim = Ratchet::NewSim();
		// Skyrim has no placed ledges or Swingshot targets: only rays, for lips he catches on
		// the way down.
		ratchet_set_world(g.sim, RatchetWorld{ .user = nullptr, .ledges = nullptr, .raycast = WorldRay, .swing_targets = nullptr });
		Input::LogBindings();

		REL::Relocation<std::uintptr_t> jumpVtbl{ RE::VTABLE_JumpHandler[0] };
		JumpHandlerHook::_CanProcess = jumpVtbl.write_vfunc(0x1, JumpHandlerHook::CanProcess);

		if (DashOnSprint()) {
			REL::Relocation<std::uintptr_t> sprintVtbl{ RE::VTABLE_SprintHandler[0] };
			SprintHandlerHook::_CanProcess = sprintVtbl.write_vfunc(0x1, SprintHandlerHook::CanProcess);
		}

		REL::Relocation<std::uintptr_t> playerVtbl{ RE::VTABLE_PlayerCharacter[0] };
		PlayerUpdateHook::_Update = playerVtbl.write_vfunc(0xAD, PlayerUpdateHook::Update);

		SKSE::log::info("jump: Skyrim's jump is off, libratchet's is on{}",
			Settings::Get().dashControl.empty() ? "" :
			DashOnSprint()                      ? "; Sprint dashes (Skyrim's sprint is off on foot)" :
			                                      std::format("; {} dashes", Settings::Get().dashControl));
	}

	bool OnFoot()
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		const auto* ctrl = player ? player->GetCharController() : nullptr;
		return g.sim && ctrl && OnFootInControl(player, ctrl);
	}

	void Reset()
	{
		if (g.sim) {
			ratchet_reset(g.sim);
		}
		const auto* player = RE::PlayerCharacter::GetSingleton();
		EndOurs(player ? player->GetCharController() : nullptr);
		g.last = {};
	}
}

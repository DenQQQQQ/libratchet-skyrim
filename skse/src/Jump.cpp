// SPDX-License-Identifier: GPL-3.0-or-later
#include "Jump.h"

#include "AutoTest.h"
#include "Camera.h"
#include "Combat.h"
#include "Fx.h"
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

		// Moving on the ground, ground this close under his feet still holds him (metres × scale,
		// the sandbox's STEP_HEIGHT): he's pressed down onto it, but a slope or a bump going down
		// falls away a little each frame.
		constexpr float kDashGroundReach = 0.35f;
		// His footprint's half width, where the ground under him is looked for (metres × scale,
		// the sandbox's BODY_HALF_WIDTH).
		constexpr float kFootprint = 0.35f;
		// A fall off a bump or a step shorter than this (seconds) lands without a sound: it shows
		// no landing either (Model's kBriefFall).
		constexpr float kBriefFall = 0.15f;
		// Moving on the ground, he runs up a step this high (metres × scale, the sandbox's
		// STEP_HEIGHT: its box move steps onto anything this high), looked for as far ahead as he
		// gets in kStepLook (seconds): he rises onto it on the way.
		constexpr float kStepUp = 0.35f;
		constexpr float kStepLook = 0.12f;
		// The steepest ground he runs along (its normal's height: 53 degrees).
		constexpr float kWalkable = 0.6f;

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
			float             airborne = 0.0f;        // seconds the sim has had him off the ground
			std::optional<float> lastGround;          // the ground's distance the sim stepped with, metres
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

		// Rift Apart's sounds for this step's moves (the game's own events for them). With his
		// clips sounding his steps, efforts, swishes and swings (Model::ClipSounds), only what the
		// sandbox plays from the moves themselves: the jump's push-off, the landing, the dashes,
		// the heli-pack and the slam.
		void MoveSounds(RE::PlayerCharacter* a_player, const RatchetStepResult& a_r, std::uint32_t a_before, float a_airborne)
		{
			const RE::NiPoint3 at = a_player->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, 1.0f * UnitsPerMetre() };
			const auto e = a_r.events;
			if (Model::ClipSounds()) {
				if (e & RATCHET_EVENT_JUMPED) {
					Sounds::Play("footstep_jump", at);
				}
				if (e & RATCHET_EVENT_LANDED && !(a_before == RATCHET_STATE_FALL && a_airborne <= kBriefFall)) {
					Sounds::Play("land", at);
					Sounds::Play("footstep_land", at);
				}
				if (e & RATCHET_EVENT_DASHED) {
					Sounds::Play("phantom_dash", at);
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
				if (e & RATCHET_EVENT_SMASH_IMPACT) {
					Sounds::Play("wrench_slam", at);
				}
				return;
			}
			if (e & (RATCHET_EVENT_JUMPED | RATCHET_EVENT_WALL_JUMPED | RATCHET_EVENT_LEDGE_JUMPED)) {
				Sounds::Play("jump", at);
				Sounds::Play("jump_swish", at);
				Sounds::Play("footstep_jump", at);
				Sounds::Play("voice_jump", at);
			}
			if (e & RATCHET_EVENT_DOUBLE_JUMPED) {
				Sounds::Play("double_jump", at);
			}
			if (e & RATCHET_EVENT_LANDED && !(a_before == RATCHET_STATE_FALL && a_airborne <= kBriefFall)) {
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

		struct Along
		{
			RE::NiPoint3 velocity;                     // Skyrim's axes, m/s
			RE::NiPoint3 normal{ 0.0f, 0.0f, 1.0f };   // the ground's he goes along
			float        step = 0.0f;                  // the step ahead he's rising onto, metres (0: none)
		};
		Along g_along;  // this frame's, for the trace

		// Moving on the ground (the sim's run, a dash, a swing), he goes along it as the sandbox's
		// box move keeps him on its floors: up and down a slope at the speed across the ground he'd
		// have on the flat, held to it by a press into it along its normal, and up a step he comes
		// to, rising onto it on the way. (Pressed straight down, the sim's 2 m/s slid him back down
		// slopes, 1 m/s on stairs' 40 degrees, and the speed the controller kept, which the sim
		// goes on from, shrank frame by frame: he stalled at the foot of the stairs.)
		// a_v: the sim's velocity in Skyrim's axes; its downward part is the press.
		Along AlongGround(const RE::bhkCharacterController* a_ctrl, const RE::NiPoint3& a_feet, const RE::NiPoint3& a_v, float a_dt)
		{
			Along out{ a_v };
			const float s = Settings::Get().scale;
			const float u = UnitsPerMetre();
			// The ground as his capsule rests on it (it can be a stair's helper ramp).
			const auto& sn = a_ctrl->supportNorm.quad.m128_f32;
			if (a_ctrl->flags.any(RE::CHARACTER_FLAGS::kSupport) && sn[2] >= kWalkable && sn[2] <= 1.0001f) {
				out.normal = { sn[0], sn[1], sn[2] };
			}
			const RE::NiPoint3& n = out.normal;
			RE::NiPoint3 v{ a_v.x, a_v.y, -(n.x * a_v.x + n.y * a_v.y) / n.z };
			// A step ahead: the first ground higher than the slope he's on, out to where he'll be
			// in kStepLook (from half his footprint out: one under his front is risen onto now).
			const float speed = std::sqrt(a_v.x * a_v.x + a_v.y * a_v.y);
			if (speed > 0.2f) {
				const RE::NiPoint3 dir{ a_v.x / speed, a_v.y / speed, 0.0f };
				const float rise = -(n.x * dir.x + n.y * dir.y) / n.z;  // the slope's, per metre ahead
				const float r = kFootprint * s;
				const float top = (kStepUp + 0.05f) * s;
				for (float d = 0.5f * r; d <= r + speed * kStepLook; d += 0.1f * s) {
					const RE::NiPoint3 at = a_feet + dir * (d * u);
					const auto hit = Physics::Ray(at + RE::NiPoint3{ 0.0f, 0.0f, top * u }, at - RE::NiPoint3{ 0.0f, 0.0f, 0.05f * u });
					if (!hit) {
						continue;
					}
					const float h = (hit->point.z - a_feet.z) / u;
					if (h - rise * d < 0.03f * s) {
						continue;  // the slope's own height, or lower
					}
					if (h > kStepUp * s || hit->normal.z < kWalkable) {
						break;  // a wall: the controller stops him at it
					}
					// Up onto it by the time his front gets there.
					const float time = std::max(a_dt, (d - r) / speed);
					v.z = std::max(v.z, h / time);
					out.step = h;
					break;
				}
			}
			if (out.step == 0.0f) {
				const float press = std::max(0.0f, -a_v.z);
				v = v - n * press;
			}
			out.velocity = v;
			return out;
		}

		// A ground move Skyrim's ground state carries out (GroundStateHook): the sim's velocity
		// across the ground for his controller (Skyrim's axes, m/s), and what the state made of it.
		struct Drive
		{
			const RE::bhkCharacterController* ctrl = nullptr;
			bool                              active = false;
			RE::NiPoint2                      velocity;
			RE::NiPoint3                      ground{ 0.0f, 0.0f, 1.0f };  // the ground's normal under him, as the rays find it
			RE::NiPoint2                      local;  // the velocityMod it got: right, forward
			RE::NiPoint3                      out;    // its outVelocity after
		};
		Drive g_drive;

		// The ground as Skyrim's ground state sees it under him (its normal: up if there's none).
		RE::NiPoint3 GroundNormal(const RE::bhkCharacterController* a_ctrl)
		{
			const auto& info = a_ctrl->surfaceInfo;
			const auto& n = info.supportedState.all(RE::hkpSurfaceInfo::SupportedState::kSupported) ? info.surfaceNormal.quad.m128_f32 :
			                a_ctrl->flags.any(RE::CHARACTER_FLAGS::kSupport)                    ? a_ctrl->supportNorm.quad.m128_f32 :
			                                                                                       nullptr;
			if (!n || !(n[2] > 0.0f && n[2] <= 1.0001f)) {
				return { 0.0f, 0.0f, 1.0f };
			}
			return { n[0], n[1], n[2] };
		}

		// The sim's velocity as the ground state takes it: velocityMod, in his own frame (x right,
		// y forward; his controller's forwardVec points back), m/s. The state keeps that speed
		// along the ground: up or down a slope he'd go slower across it than the sim, so it's
		// longer by the slope's length per metre across (measured on the 30 degree ramp: 5.74 m/s
		// across it asked, 5.7 m/s made). The slope as the rays find it: the state's own contacts
		// on stairs are their steps' edges, steep, and longer by those it went faster than asked.
		void DriveVelocity(RE::bhkCharacterController* a_ctrl)
		{
			const auto& f = a_ctrl->forwardVec.quad.m128_f32;
			const float fl = std::sqrt(f[0] * f[0] + f[1] * f[1]);
			if (fl < 1e-4f) {
				return;
			}
			const float fx = -f[0] / fl, fy = -f[1] / fl;
			float       vx = g_drive.velocity.x, vy = g_drive.velocity.y;
			const float speed = std::sqrt(vx * vx + vy * vy);
			const RE::NiPoint3& n = g_drive.ground;
			if (speed > 1e-3f && n.z >= kWalkable && n.z <= 1.0001f) {
				const float t = -(n.x * vx + n.y * vy) / (n.z * speed);
				const float k = std::min(std::sqrt(1.0f + t * t), 1.7f);
				vx *= k;
				vy *= k;
			}
			g_drive.local = { vx * fy - vy * fx, vx * fx + vy * fy };
			a_ctrl->velocityMod = RE::hkVector4(g_drive.local.x, g_drive.local.y, 0.0f, 0.0f);
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
			// The ground state moves him on its own unless this step hands it the sim's velocity.
			g_drive.active = false;
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
				Combat::OffFoot(a_player, a_dt);
				return;
			}

			const float z = a_player->GetPositionZ();
			const RE::NiPoint3 feet = a_player->GetPosition();
			// How far down the ground is, under his footprint as the sandbox measures it: the sim's
			// jumps, double jump, glide and falls ask. (A single ray from his middle missed bumps
			// he stood on, or fell into the cracks between them.)
			const float s = Settings::Get().scale;
			RE::NiPoint3 groundNormal{ 0.0f, 0.0f, 1.0f };
			const auto   ground = Physics::GroundUnder(feet, 20.0f, kFootprint * s, &groundNormal);
			// Skyrim's controller says whether he stands. In a ground move it's kept in its air
			// state, and then the ground right under his feet is what holds him up.
			// (Not second-guessed by the ray: on a bumpy road it often finds nothing under his feet,
			// and a flight that ignored Skyrim's "standing" there slid him along the bumps, seconds
			// at a time. Skyrim's capsule standing on an edge at his chest is a landing too.)
			const bool grounded = Grounded(ctrl) || (g.groundMove && ground && *ground < kDashGroundReach * s);
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
				// In the sim's own walk, only when he went slower than it asked (a wall, or someone
				// in the way): up stairs Skyrim moves him faster at times, and its steps' edges turn
				// him, and fed back each frame that ran away with him, off the stairs' side.
				const RatchetVec3 v = ControllerVelocity(ctrl);
				const float       asked = g.last.velocity.x * g.last.velocity.x + g.last.velocity.z * g.last.velocity.z;
				const float       made = v.x * v.x + v.z * v.z;
				if (!g.ours || made < 0.9f * asked) {
					ratchet_set_velocity(g.sim, { v.x, g.last.velocity.y, v.z });
				}
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
			const float airborne = g.airborne;
			g.lastGround = contact.has_ground_distance ? std::optional(contact.ground_distance) : std::nullopt;
			g.airborne = r.state != RATCHET_STATE_GROUND ? g.airborne + a_dt : 0.0f;
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

			if (g.groundMove && Settings::Get().nativeGround && Grounded(ctrl)) {
				// Skyrim's ground state moves him at the sim's speed (GroundStateHook): along slopes,
				// over small edges and up stairs, held to the ground by its own pull and snap.
				g_drive.ctrl = ctrl;
				g_drive.active = true;
				g_drive.velocity = { r.velocity.x, -r.velocity.z };
				g_drive.ground = ground && *ground < kDashGroundReach * s ? groundNormal : RE::NiPoint3{ 0.0f, 0.0f, 1.0f };
				ctrl->gravity = g.savedGravity;
				g_along = {};
				ctrl->fallStartHeight = z;
				ctrl->fallTime = 0.0f;
			} else if (g.groundMove) {
				// In the air state with the sim's velocity (see StartGroundMove): bumps along the way
				// (Skyrim's ground state lost him, the ground's still in reach) aren't falls.
				ctrl->context.currentState = RE::hkpCharacterStateType::kInAir;
				ctrl->gravity = 0.0f;
				RatchetVec3 v = r.velocity;
				if (grounded) {
					// Along the ground: up and down slopes, up steps.
					g_along = AlongGround(ctrl, feet, { v.x, -v.z, v.y }, a_dt);
					v = { g_along.velocity.x, g_along.velocity.z, -g_along.velocity.y };
				} else {
					g_along = {};
				}
				// Down a step the ground falls away under him: he follows it while it's in reach
				// (Skyrim's ground state would snap him down too).
				if (g_along.step == 0.0f && ground && *ground > 0.02f && *ground < kDashGroundReach * s) {
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
			if (r.events & RATCHET_EVENT_SMASH_IMPACT) {
				Camera::Wobble(0.25f);  // the sandbox's, on the slam
			}
			if (r.events & RATCHET_EVENT_MELEE_SWING) {
				AutoTest::OnMove(std::format("swing{}", r.melee_swing + 1));
			}
			MoveSounds(a_player, r, g.last.state, airborne);
			// The wrench's hits, the slam, the gun.
			Combat::Step(a_player, g.sim, r, a_dt);
			if (Settings::Get().traceJumps && (g.ours || in.jump || in.dash || r.events || AutoTest::Active())) {
				SKSE::log::info(
					"trace: dt {:.4f} jump {} dash {} grounded {} ground {}{:.2f} ceiling {} stick {:.2f},{:.2f} -> state {} events {:#x} v {:.2f},{:.3f},{:.2f} facing {:.2f} z {:.1f} ctrl {}{}",
					a_dt, in.jump, in.dash, contact.grounded, contact.has_ground_distance ? "" : "none ", contact.ground_distance, contact.hit_ceiling,
					in.move_x, in.move_z, r.state, r.events, r.velocity.x, r.velocity.y, r.velocity.z, r.facing_yaw, z,
					static_cast<int>(ctrl->context.currentState),
					g_drive.active ? std::format(" native mod {:.2f},{:.2f} last out {:.2f},{:.2f},{:.2f} normal {:.2f},{:.2f},{:.2f}", g_drive.local.x,
										 g_drive.local.y, g_drive.out.x, g_drive.out.y, g_drive.out.z, GroundNormal(ctrl).x, GroundNormal(ctrl).y,
										 GroundNormal(ctrl).z) :
					g.groundMove ? std::format(" support {} {:.2f},{:.2f},{:.2f}{} along {:.2f},{:.2f},{:.2f} step {:.2f}",
									   ctrl->flags.any(RE::CHARACTER_FLAGS::kSupport) ? "yes" : "no", ctrl->supportNorm.quad.m128_f32[0],
									   ctrl->supportNorm.quad.m128_f32[1], ctrl->supportNorm.quad.m128_f32[2],
									   ctrl->flags.any(RE::CHARACTER_FLAGS::kOnStairs) ? " stairs" : "", g_along.velocity.x, g_along.velocity.y,
									   g_along.velocity.z, g_along.step) :
								   "");
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

		// Skyrim's ground state as it moves a character this frame (its physics step, vfunc 8 of
		// bhkCharacterStateOnGround): it moves him at velocityMod along the ground he stands on. In
		// a ground move the sim's velocity goes in there. (Set from a game update it doesn't stick:
		// the locomotion's is written over it before this step.) It runs for every character, maybe
		// off the main thread: only his controller is touched.
		struct GroundStateHook
		{
			static void Simulate(RE::bhkCharacterStateOnGround* a_this, RE::bhkCharacterController* a_ctrl)
			{
				const bool ours = a_ctrl && g_drive.active && a_ctrl == g_drive.ctrl;
				if (ours) {
					DriveVelocity(a_ctrl);
				}
				_Simulate(a_this, a_ctrl);
				if (ours) {
					const auto& o = a_ctrl->outVelocity.quad.m128_f32;
					g_drive.out = { o[0], o[1], o[2] };
				}
			}
			static inline REL::Relocation<decltype(Simulate)> _Simulate;
		};

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
												  .hasFacing = true, .facingYaw = g.last.facing_yaw, .sim = g.sim, .groundDistance = g.lastGround });
					// The weapons' effects, after his pose (they start at his gun's muzzle, and the
					// swing streak follows the wrench in his hand).
					const auto state = g.last.state;
					const bool swinging = state == RATCHET_STATE_MELEE || state == RATCHET_STATE_SMASH || state == RATCHET_STATE_SMASH_LAND;
					Fx::Swipe(swinging ? Model::HeldWrench() : std::nullopt);
					Fx::Update(dt);
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

		REL::Relocation<std::uintptr_t> groundVtbl{ RE::VTABLE_bhkCharacterStateOnGround[0] };
		GroundStateHook::_Simulate = groundVtbl.write_vfunc(0x8, GroundStateHook::Simulate);

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

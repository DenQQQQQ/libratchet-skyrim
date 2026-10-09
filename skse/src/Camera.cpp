// SPDX-License-Identifier: GPL-3.0-or-later
#include "Camera.h"

#include "Anim.h"
#include "Combat.h"
#include "Jump.h"
#include "Physics.h"
#include "Settings.h"

#include <nlohmann/json.hpp>

namespace Camera
{
	namespace
	{
		using Json = nlohmann::json;

		float Units() { return RE::bhkWorld::GetWorldScaleInverse(); }

		constexpr float kPi = 3.14159265f;
		constexpr float kTau = 2.0f * kPi;
		constexpr float kDegrees = kPi / 180.0f;
		// camera.rs: the pitch (radians, the camera above him = positive) at which the looking-down
		// offset applies fully, the (negative) one for looking up, and the speeds (m/s at Ratchet's
		// size) between which the camera moves to its running place. The sandbox's own.
		constexpr float kPitchDown = 1.2f;
		constexpr float kPitchUp = -0.6f;
		constexpr float kTraversalFrom = 2.5f;
		constexpr float kTraversalTo = 6.0f;
		// main.rs camera_follow: the focus closes the gap to him at this rate (an exponent per
		// second, every way off the Swingshot); aiming blends its framing in over 0.13 s, running
		// over 0.5 s.
		constexpr float kFollowGain = -10.0f;
		constexpr float kAimTime = 0.13f;
		constexpr float kMovingTime = 0.5f;
		// Ours: the sandbox's camera goes through walls; this one stays this far off them (metres ×
		// scale), and no nearer his pivot than kNearest.
		constexpr float kWallMargin = 0.2f;
		constexpr float kNearest = 0.4f;

		// One camera context (camera.rs CameraContext). Offsets from the pivot, metres at
		// Ratchet's size: x to his right, y up, z back.
		struct Context
		{
			float        pivot;
			RE::NiPoint3 level, down, up;
			bool         traversal;
			float        traversalPivot;
			RE::NiPoint3 traversalLevel;
			float        fov;  // horizontal, degrees
			float        traversalFov;

			struct Place
			{
				float        pivot;
				RE::NiPoint3 offset;
				float        fov;
			};

			// Pivot height, offset and field of view for a camera pitch and how far into moving
			// fast he is (0..1).
			Place At(float a_pitch, float a_moving) const
			{
				const bool         downward = a_pitch >= 0.0f;
				const RE::NiPoint3 key = downward ? down : up;
				const float        k = std::clamp(downward ? a_pitch / kPitchDown : a_pitch / kPitchUp, 0.0f, 1.0f);
				RE::NiPoint3       offset = level + (key - level) * k;
				float              height = pivot;
				if (traversal) {
					offset += (traversalLevel - level) * a_moving;
					height += (traversalPivot - pivot) * a_moving;
				}
				return { height, offset, fov + (traversalFov - fov) * a_moving };
			}
		};

		// camera.rs CameraConfigs::generic(): the default and strafe (aiming) contexts.
		Context g_stand{ 1.4f, { 0.0f, 0.0f, 5.0f }, { 0.0f, 0.4f, 5.0f }, { 0.0f, 0.5f, 3.0f }, true, 1.5f, { 0.0f, 0.2f, 6.0f }, 75.0f, 80.0f };
		Context g_aim{ 1.5f, { 0.6f, 0.0f, 4.0f }, { 0.6f, 0.0f, 3.0f }, { 0.8f, 0.0f, 1.5f }, false, 0.0f, {}, 70.0f, 70.0f };
		const char* g_source = "generic";

		// A camera shake (camera.rs Shake): per axis, three sine curves (magnitude, frequency in
		// Hz, phase in turns), full for its duration, then fading out; read as degrees of yaw,
		// pitch and roll.
		struct Shake
		{
			float curves[3][3][3]{};  // axis, curve, (magnitude, frequency, phase)
			float duration = 0.0f;
			float fadeout = 0.0f;
			bool  loaded = false;

			std::optional<RE::NiPoint3> Sample(float a_t) const
			{
				if (a_t > duration + fadeout) {
					return std::nullopt;
				}
				const float envelope = a_t <= duration ? 1.0f : 1.0f - (a_t - duration) / std::max(fadeout, 1e-4f);
				float       v[3]{};
				for (int axis = 0; axis < 3; ++axis) {
					for (const auto& c : curves[axis]) {
						v[axis] += c[0] * std::sin(kTau * (c[1] * a_t + c[2]));
					}
				}
				return RE::NiPoint3{ v[0], v[1], v[2] } * envelope;
			}
		};
		Shake g_recoil;
		Shake g_blast;
		float g_blastInner = 30.0f;  // metres × scale (wpn_protogrenade_weapon.json ShakeData)
		float g_blastOuter = 50.0f;

		struct Playing
		{
			const Shake* shake;
			float        age;
			float        strength;
		};

		struct Rig
		{
			bool                 active = false;
			RE::NiPoint3         focus;          // game units: where the pivot follows him
			float                aim = 0.0f;     // 0..1: over his shoulder for aiming
			float                moving = 0.0f;  // 0..1: back and wider for running
			std::vector<Playing> shakes;
			float                wobble = 0.0f;  // seconds left
			float                clock = 0.0f;
			RE::NiQuaternion     turn{ 1.0f, 0.0f, 0.0f, 0.0f };  // this frame's shake, in the camera's own frame
			float                savedFov = 0.0f;                 // Skyrim's own, given back when ours stops
			float                fovScale = 0.0f;                 // tan(vertical/2) / tan(Skyrim's FOV/2), measured
		};
		Rig g;

		// --- configs

		float Num(const Json& a_v, const char* a_key, float a_default)
		{
			const auto it = a_v.find(a_key);
			return it != a_v.end() && it->is_number() ? it->get<float>() : a_default;
		}

		// The game's offsets are x to the left and z forward.
		std::optional<RE::NiPoint3> Offset(const Json& a_v, const char* a_key)
		{
			const auto it = a_v.find(a_key);
			if (it == a_v.end() || !it->is_object()) {
				return std::nullopt;
			}
			return RE::NiPoint3{ -Num(*it, "X", 0.0f), Num(*it, "Y", 0.0f), -Num(*it, "Z", 0.0f) };
		}

		std::optional<Json> Read(const std::filesystem::path& a_file)
		{
			std::ifstream in(a_file);
			if (!in) {
				return std::nullopt;
			}
			auto v = Json::parse(in, nullptr, false);
			if (v.is_discarded()) {
				return std::nullopt;
			}
			return v;
		}

		std::optional<Json> ReadContext(const std::filesystem::path& a_file)
		{
			auto v = Read(a_file);
			if (!v || v->value("_type", "") != "CameraAimContextConfig") {
				return std::nullopt;
			}
			return v;
		}

		// camera.rs CameraContext::from_json.
		Context FromJson(const Json& a_v, const Context& a_base)
		{
			Context c = a_base;
			c.level = Offset(a_v, "StandOffsetLevel").value_or(a_base.level);
			c.down = Offset(a_v, "StandOffsetDown").value_or(c.level);
			c.up = Offset(a_v, "StandOffsetUp").value_or(c.level);
			c.pivot = Num(a_v, "StandPivotHeight", a_base.pivot);
			if (const auto traversal = Offset(a_v, "TraversalOffsetLevel")) {
				c.traversal = true;
				c.traversalLevel = *traversal;
				c.traversalPivot = Num(a_v, "TraversalPivotHeight", a_base.pivot);
			} else {
				c.traversal = false;
			}
			c.fov = Num(a_v, "FieldOfViewStand", a_base.fov);
			c.traversalFov = Num(a_v, "FieldOfViewTraversal", c.fov);
			return c;
		}

		Shake LoadShake(const std::filesystem::path& a_file)
		{
			Shake      s;
			const auto v = Read(a_file);
			if (!v || v->value("_type", "") != "CameraShakingConfig" || !v->contains("Data")) {
				return s;
			}
			const auto& d = (*v)["Data"];
			const char* axes[] = { "CurveDataX", "CurveDataY", "CurveDataZ" };
			const char* curves[] = { "CurveOne", "CurveTwo", "CurveThree" };
			for (int a = 0; a < 3; ++a) {
				const auto axis = d.find(axes[a]);
				if (axis == d.end()) {
					continue;
				}
				for (int c = 0; c < 3; ++c) {
					const auto curve = axis->find(curves[c]);
					if (curve != axis->end()) {
						s.curves[a][c][0] = Num(*curve, "Magnitude", 0.0f);
						s.curves[a][c][1] = Num(*curve, "Frequency", 0.0f);
						s.curves[a][c][2] = Num(*curve, "Offset", 0.0f);
					}
				}
			}
			s.duration = Num(d, "Duration", 0.0f);
			s.fadeout = Num(d, "FadeoutDuration", 0.0f);
			s.loaded = true;
			return s;
		}

		// The first object under a_v with a_key (depth first).
		const Json* FindKey(const Json& a_v, const char* a_key)
		{
			if (a_v.is_object()) {
				if (const auto it = a_v.find(a_key); it != a_v.end()) {
					return &*it;
				}
			}
			if (a_v.is_structured()) {
				for (const auto& child : a_v) {
					if (const Json* found = FindKey(child, a_key)) {
						return found;
					}
				}
			}
			return nullptr;
		}

		// --- each frame

		bool Wanted() { return Settings::Get().riftCamera && Jump::OnFoot(); }

		void Stop()
		{
			if (!g.active) {
				return;
			}
			g.active = false;
			g.shakes.clear();
			g.turn = { 1.0f, 0.0f, 0.0f, 0.0f };
			if (auto* camera = RE::PlayerCamera::GetSingleton(); camera && g.savedFov > 0.0f) {
				camera->GetRuntimeData2().worldFOV = g.savedFov;
			}
		}

		// Skyrim's FOV value for a horizontal field of view on this screen. Skyrim's value isn't the
		// screen's horizontal angle (its own convention): the world camera's frustum, measured as
		// ours starts, says how its tangent relates to the vertical one.
		float SkyrimFov(float a_horizontal)
		{
			auto* world = RE::Main::WorldRootCamera();
			if (!world) {
				return a_horizontal;
			}
			const auto& f = world->GetRuntimeData2().viewFrustum;
			const float aspect = f.fTop > 1e-4f ? f.fRight / f.fTop : 16.0f / 9.0f;
			const float tanVertical = std::tan(a_horizontal * 0.5f * kDegrees) / aspect;
			return 2.0f * std::atan(tanVertical / g.fovScale) / kDegrees;
		}

		void Start(RE::PlayerCamera* a_camera, const RE::NiPoint3& a_feet)
		{
			g.active = true;
			g.focus = a_feet;
			g.savedFov = a_camera->GetRuntimeData2().worldFOV;
			g.fovScale = 0.0f;
			if (auto* world = RE::Main::WorldRootCamera(); world && g.savedFov > 1.0f) {
				const auto& f = world->GetRuntimeData2().viewFrustum;
				g.fovScale = f.fTop / std::tan(g.savedFov * 0.5f * kDegrees);
				static bool logged = false;
				if (!logged) {
					logged = true;
					SKSE::log::info("camera: Skyrim's FOV {:.1f} draws {:.1f} across, {:.1f} up ({}): its vertical tangent is {:.3f} of its FOV's",
						g.savedFov, 2.0f * std::atan(f.fRight) / kDegrees, 2.0f * std::atan(f.fTop) / kDegrees,
						f.fTop > 1e-4f ? std::format("{:.3f} wide", f.fRight / f.fTop) : "?", g.fovScale);
				}
			}
			if (!(g.fovScale > 0.1f && g.fovScale < 10.0f)) {
				g.fovScale = 0.75f;  // Skyrim's FOV read as 4:3's horizontal (the usual reading)
			}
		}

		// Rift Apart's place for the camera: where Skyrim's would be (a_translation) is replaced.
		// a_rotation: Skyrim's turn for it (its mouse look), without our shake.
		void Place(const RE::NiQuaternion& a_rotation, RE::NiPoint3& a_translation)
		{
			auto* player = RE::PlayerCharacter::GetSingleton();
			auto* camera = RE::PlayerCamera::GetSingleton();
			if (!player || !camera || !Wanted()) {
				Stop();
				return;
			}
			const float        dt = std::clamp(RE::GetSecondsSinceLastFrame(), 0.0f, 0.1f);
			const float        s = Settings::Get().scale;
			const float        u = Units();
			const RE::NiPoint3 feet = player->GetPosition();
			if (!g.active) {
				Start(camera, feet);
			} else if (g.focus.GetDistance(feet) > 10.0f * s * u) {
				g.focus = feet;  // carried somewhere else: the camera goes with him at once
			}
			g.focus += (feet - g.focus) * (1.0f - std::exp(kFollowGain * dt));

			// Where Skyrim's camera looks: its pitch the sandbox's way (above him = positive), and
			// its right along the ground.
			const RE::NiMatrix3 turn = a_rotation.ToRotation();
			const RE::NiPoint3  forward = turn.GetVectorY();
			const float         pitch = std::asin(std::clamp(-forward.z, -1.0f, 1.0f));
			RE::NiPoint3        right{ forward.y, -forward.x, 0.0f };
			const float         rl = right.Length();
			right = rl > 1e-4f ? right / rl : turn.GetVectorX();

			// Aiming blends to the strafe framing; running, to the running place.
			g.aim += ((Combat::ForAnimation().aiming ? 1.0f : 0.0f) - g.aim) * (1.0f - std::exp(-dt / kAimTime));
			float speed = 0.0f;
			if (const auto* ctrl = player->GetCharController()) {
				RE::hkVector4 v;
				ctrl->GetLinearVelocityImpl(v);
				speed = std::sqrt(v.quad.m128_f32[0] * v.quad.m128_f32[0] + v.quad.m128_f32[1] * v.quad.m128_f32[1]) / s;
			}
			const float running = std::clamp((speed - kTraversalFrom) / (kTraversalTo - kTraversalFrom), 0.0f, 1.0f);
			g.moving += (running - g.moving) * (1.0f - std::exp(-dt / kMovingTime));

			// The wobble (the Hyper-Strike's landing): 20 Hz, fading out over its last 0.25 s.
			g.clock += dt;
			g.wobble = std::max(0.0f, g.wobble - dt);
			const float wobble = std::sin(g.clock * 20.0f * kTau) * std::min(g.wobble / 0.25f, 1.0f);

			const auto         stand = g_stand.At(pitch + wobble * 0.004f, g.moving);
			const auto         aim = g_aim.At(pitch + wobble * 0.004f, 0.0f);
			const float        b = g.aim;
			const float        pivot = (stand.pivot + (aim.pivot - stand.pivot) * b) * s * u;
			const RE::NiPoint3 offset = (stand.offset + (aim.offset - stand.offset) * b) * (s * u);
			const RE::NiPoint3 up{ 0.0f, 0.0f, 1.0f };
			const RE::NiPoint3 focus = g.focus + up * (pivot + offset.y) + right * offset.x;
			RE::NiPoint3       at = focus - forward * offset.z;
			// Not through walls: from his pivot out to there, on Skyrim's camera layer.
			const RE::NiPoint3 from = g.focus + up * pivot;
			if (const auto hit = Physics::Ray(from, at, RE::COL_LAYER::kCameraSphere)) {
				const RE::NiPoint3 way = at - from;
				const float        len = way.Length();
				if (len > 1e-3f) {
					const float keep = std::max(hit->fraction * len - kWallMargin * s * u, std::min(kNearest * s * u, len));
					at = from + way * (keep / len);
				}
			}
			a_translation = at;
			camera->GetRuntimeData2().worldFOV = SkyrimFov(stand.fov + (aim.fov - stand.fov) * b);

			// The shakes, added up (degrees of yaw, pitch and roll), and the wobble's turn: the view
			// turned by them in its own frame (Skyrim's camera: x right, y forward, z up; its roll
			// about y turns the other way from the sandbox's about its back).
			RE::NiPoint3 shake{};
			std::erase_if(g.shakes, [&](Playing& a_p) {
				a_p.age += dt;
				const auto v = a_p.shake->Sample(a_p.age);
				if (v) {
					shake += *v * a_p.strength;
				}
				return !v;
			});
			const float yaw = shake.x * kDegrees + wobble * 0.012f;
			const float tilt = shake.y * kDegrees + wobble * 0.004f;
			const float roll = -shake.z * kDegrees;
			const auto  about = [](float a_angle, float a_x, float a_y, float a_z) {
                const float h = 0.5f * a_angle;
                return RE::NiQuaternion{ std::cos(h), a_x * std::sin(h), a_y * std::sin(h), a_z * std::sin(h) };
			};
			g.turn = Anim::Multiply(Anim::Multiply(about(yaw, 0.0f, 0.0f, 1.0f), about(tilt, 1.0f, 0.0f, 0.0f)), about(roll, 0.0f, 1.0f, 0.0f));
		}

		// Skyrim's third-person camera state: where it puts the camera, and how it turns it.
		struct ThirdPersonHook
		{
			static void GetRotation(RE::ThirdPersonState* a_this, RE::NiQuaternion& a_rotation)
			{
				_GetRotation(a_this, a_rotation);
				if (g.active) {
					a_rotation = Anim::Multiply(a_rotation, g.turn);
				}
			}

			// Its place isn't asked for through its vtable (only its turn is): after its update, ours
			// goes where Skyrim's went, into the state and onto the camera's root.
			static void Update(RE::ThirdPersonState* a_this, RE::BSTSmartPointer<RE::TESCameraState>& a_next)
			{
				_Update(a_this, a_next);
				RE::NiQuaternion look;
				_GetRotation(a_this, look);
				RE::NiPoint3 at = a_this->translation;
				Place(look, at);
				if (!g.active) {
					return;
				}
				a_this->translation = at;
				if (auto* camera = RE::PlayerCamera::GetSingleton(); camera && camera->cameraRoot) {
					camera->cameraRoot->local.translate = at;
					RE::NiUpdateData update{ 0.0f, RE::NiUpdateData::Flag::kNone };
					camera->cameraRoot->Update(update);
				}
			}

			static inline REL::Relocation<decltype(GetRotation)> _GetRotation;
			static inline REL::Relocation<decltype(Update)>      _Update;
		};
	}

	void Install(const std::filesystem::path& a_configs)
	{
		if (const auto v = ReadContext(a_configs / "camera_Default_AimContext.json")) {
			g_stand = FromJson(*v, g_stand);
			if (const auto aim = ReadContext(a_configs / "camera_Strafe_AimContext.json")) {
				g_aim = FromJson(*aim, g_aim);
			}
			g_source = "Rift Apart's camera configs";
		}
		g_recoil = LoadShake(a_configs / "camerashake_recoilsmall.json");
		g_blast = LoadShake(a_configs / "camera_CameraShake_MediumExplosion.json");
		if (const auto grenade = Read(a_configs / "wpn_protogrenade_weapon.json")) {
			if (const Json* shake = FindKey(*grenade, "ShakeData")) {
				g_blastInner = Num(*shake, "InnerRadius", g_blastInner);
				g_blastOuter = Num(*shake, "OuterRadius", g_blastOuter);
			}
		}
		SKSE::log::info("camera: {} (pivot {:.2f} m, {:.2f} m back, FOV {:.0f}; running {:.2f} m back, FOV {:.0f}; aiming {:.2f} m right, {:.2f} m back, FOV {:.0f}); shakes: recoil {}, blast {} ({:.0f}-{:.0f} m)",
			g_source, g_stand.pivot, g_stand.level.z, g_stand.fov, g_stand.traversalLevel.z, g_stand.traversalFov, g_aim.level.x, g_aim.level.z, g_aim.fov,
			g_recoil.loaded ? "yes" : "no", g_blast.loaded ? "yes" : "no", g_blastInner, g_blastOuter);
		if (!Settings::Get().riftCamera) {
			return;
		}
		REL::Relocation<std::uintptr_t> vtbl{ RE::VTABLE_ThirdPersonState[0] };
		ThirdPersonHook::_Update = vtbl.write_vfunc(0x3, ThirdPersonHook::Update);
		ThirdPersonHook::_GetRotation = vtbl.write_vfunc(0x4, ThirdPersonHook::GetRotation);
	}

	void Recoil()
	{
		if (g.active && g_recoil.loaded) {
			g.shakes.push_back({ &g_recoil, 0.0f, 1.0f });
		}
	}

	bool Explosion(const RE::NiPoint3& a_at)
	{
		auto* player = RE::PlayerCharacter::GetSingleton();
		if (!g.active || !g_blast.loaded || !player) {
			return false;
		}
		const float s = Settings::Get().scale;
		const float d = player->GetPosition().GetDistance(a_at) / (Units() * s);
		const float inner = g_blastInner, outer = std::max(g_blastOuter, g_blastInner + 1e-3f);
		const float strength = d <= inner ? 1.0f : std::clamp((outer - d) / (outer - inner), 0.0f, 1.0f);
		if (strength > 0.0f) {
			g.shakes.push_back({ &g_blast, 0.0f, strength });
		}
		return true;
	}

	void Wobble(float a_seconds) { g.wobble = std::max(g.wobble, a_seconds); }
}

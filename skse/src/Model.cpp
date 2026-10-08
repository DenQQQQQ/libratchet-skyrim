// SPDX-License-Identifier: GPL-3.0-or-later
#include "Model.h"

#include "Anim.h"
#include "Settings.h"

namespace Model
{
	namespace
	{
		constexpr const char* kModelPath = "RatchetMovement\\ratchet.nif";  // under Data\meshes

		// The sandbox's clip rules (apps/sandbox/src/ratchet_model.rs), at Ratchet's size (m/s).
		constexpr float kMoving = 0.6f;
		constexpr float kRunToWalk = 2.4f;
		constexpr float kWalkToRun = 3.0f;
		constexpr float kTurnRate = 12.0f;  // rad/s he turns to face where he's going (ours)

		struct Playing
		{
			const Anim::Clip* clip = nullptr;
			float             time = 0.0f;
			float             rate = 1.0f;
			bool              loop = true;
		};

		struct State
		{
			Anim::Library             lib;
			bool                      enabled = false;
			bool                      failed = false;
			RE::NiPointer<RE::NiNode> model;
			std::vector<RE::NiAVObject*> nodes;  // by joint
			Playing                   cur, prev;
			float                     fade = 1.0f;     // how far the crossfade from prev to cur has got
			float                     fadeLen = 0.16f;
			Anim::Pose                pose, other;
			float                     yaw = 0.0f;      // his facing relative to the player's, radians
			bool                      walking = false;
			float                     landTimer = 0.0f;
			bool                      wasAir = false;
			float                     airTime = 0.0f;
			std::string               clipName;
			bool                      attachAllowed = true;
			int                       showOnly = -1;
			std::vector<RE::BSGeometry*> shapes;
		};
		State s;

		bool g_castShadows = true;

		void ApplyShadows()
		{
			for (auto* shape : s.shapes) {
				if (auto* shader = shape->GetGeometryRuntimeData().shaderProperty.get()) {
					if (g_castShadows) {
						shader->flags.set(RE::BSShaderProperty::EShaderPropertyFlag::kCastShadows);
					} else {
						shader->flags.reset(RE::BSShaderProperty::EShaderPropertyFlag::kCastShadows);
					}
				}
			}
		}

		void ApplyShapeVisibility()
		{
			for (int i = 0; i < static_cast<int>(s.shapes.size()); ++i) {
				const bool show = s.showOnly == -1 || s.showOnly == i;
				if (s.shapes[i]->GetAppCulled() == show) {
					s.shapes[i]->SetAppCulled(!show);
				}
			}
		}

		float UnitsPerMetre() { return RE::bhkWorld::GetWorldScaleInverse(); }

		bool Looping(std::string_view a_clip)
		{
			return a_clip == "idle" || a_clip == "run" || a_clip == "walk" || a_clip == "fall" || a_clip == "glide" ||
			       a_clip == "wall_slide_left" || a_clip == "wall_slide_right" || a_clip == "ledge_idle";
		}

		void Play(std::string_view a_name, float a_fade)
		{
			const Anim::Clip* clip = s.lib.Find(a_name);
			if (!clip) {
				clip = s.lib.Find("idle");
			}
			if (!clip || clip == s.cur.clip) {
				return;
			}
			s.prev = s.cur;
			s.cur = { clip, 0.0f, 1.0f, Looping(clip->name) };
			s.fade = s.prev.clip ? 0.0f : 1.0f;
			s.fadeLen = std::max(a_fade, 0.01f);
			s.clipName = clip->name;
		}

		// glTF's axes (Y up, facing +Z, metres) to Skyrim's (Z up, facing +Y), turned by a_yaw
		// about the up axis.
		RE::NiMatrix3 Orientation(float a_yaw)
		{
			const float c = std::cos(a_yaw), sn = std::sin(a_yaw);
			// Rz(yaw) * M, M = [[-1,0,0],[0,0,1],[0,1,0]].
			RE::NiMatrix3 m;
			m.entry[0][0] = -c;
			m.entry[0][1] = 0.0f;
			m.entry[0][2] = -sn;
			m.entry[1][0] = -sn;
			m.entry[1][1] = 0.0f;
			m.entry[1][2] = c;
			m.entry[2][0] = 0.0f;
			m.entry[2][1] = 1.0f;
			m.entry[2][2] = 0.0f;
			return m;
		}

		float WrapAngle(float a)
		{
			constexpr float pi = 3.14159265f;
			while (a > pi) {
				a -= 2.0f * pi;
			}
			while (a < -pi) {
				a += 2.0f * pi;
			}
			return a;
		}

		bool Attach(RE::PlayerCharacter* a_player)
		{
			auto* root = a_player->Get3D(false);
			auto* rootNode = root ? root->AsNode() : nullptr;
			if (!rootNode) {
				return false;
			}
			if (!s.model) {
				RE::NiPointer<RE::NiNode> loaded;
				const RE::BSModelDB::DBTraits::ArgsType args{};
				const auto err = RE::BSModelDB::Demand(kModelPath, loaded, args);
				if (err != RE::BSResource::ErrorCode::kNone || !loaded) {
					SKSE::log::error("ratchet model: couldn't load meshes\\{} (error {})", kModelPath, static_cast<int>(err));
					s.failed = true;
					return false;
				}
				s.model = loaded;
				s.nodes.assign(s.lib.Joints().size(), nullptr);
				std::size_t found = 0;
				for (std::size_t i = 0; i < s.lib.Joints().size(); ++i) {
					s.nodes[i] = s.model->GetObjectByName(RE::BSFixedString(s.lib.Joints()[i].name));
					found += s.nodes[i] ? 1 : 0;
				}
				SKSE::log::info("ratchet model: loaded meshes\\{}, {} of {} joints found", kModelPath, found, s.lib.Joints().size());
				s.pose = s.lib.Rest();
				RE::BSVisit::TraverseScenegraphGeometries(s.model.get(), [](RE::BSGeometry* a_geom) {
					s.shapes.push_back(a_geom);
					return RE::BSVisit::BSVisitControl::kContinue;
				});
				ApplyShapeVisibility();
				ApplyShadows();
			}
			if (s.model->parent != rootNode) {
				if (s.model->parent) {
					s.model->parent->DetachChild(s.model.get());
				}
				rootNode->AttachChild(s.model.get(), true);
				SKSE::log::info("ratchet model: attached to the player");
			}
			return true;
		}

		// The Dragonborn's own meshes stay loaded (Skyrim keeps animating him for the camera and
		// the collision) but aren't drawn.
		void HideDragonborn(RE::PlayerCharacter* a_player)
		{
			auto* rootNode = a_player->Get3D(false) ? a_player->Get3D(false)->AsNode() : nullptr;
			if (!rootNode) {
				return;
			}
			for (auto& child : rootNode->GetChildren()) {
				if (!child || child.get() == s.model.get()) {
					continue;
				}
				RE::BSVisit::TraverseScenegraphGeometries(child.get(), [](RE::BSGeometry* a_geom) {
					if (!a_geom->GetAppCulled()) {
						a_geom->SetAppCulled(true);
					}
					return RE::BSVisit::BSVisitControl::kContinue;
				});
			}
		}

		void PickClip(const Drive& a_drive, float a_speed, float a_vy, float a_dt)
		{
			const bool air = a_drive.simJump ? a_drive.simState != RATCHET_STATE_GROUND : !a_drive.grounded;
			s.airTime = air ? s.airTime + a_dt : 0.0f;
			const bool landed = !air && s.wasAir;
			s.wasAir = air && (a_drive.simJump || s.airTime > 0.15f);
			if (landed) {
				s.landTimer = a_speed > kMoving ? 0.35f : 0.3f;
			}
			s.landTimer = std::max(0.0f, s.landTimer - a_dt);

			std::string_view want;
			if (a_drive.simJump && a_drive.simState != RATCHET_STATE_GROUND) {
				switch (a_drive.simState) {
				case RATCHET_STATE_JUMP:
					want = a_vy < -5.0f ? "fall" : "jump";
					break;
				case RATCHET_STATE_DOUBLE_JUMP:
					want = "double_jump";
					break;
				case RATCHET_STATE_GLIDE:
					want = "glide";
					break;
				case RATCHET_STATE_DASH:
					want = "dash";
					break;
				case RATCHET_STATE_AIR_DASH:
					want = "air_dash";
					break;
				// Walls and ledges: the sandbox's clips, picked the simple way (its
				// ratchet_model.rs also turns him in from a fall and loops the slide).
				case RATCHET_STATE_WALL_STICK:
					want = a_drive.verticalSpeed < -0.1f ? (a_drive.wallRight ? "wall_slide_right" : "wall_slide_left") :
					                                       (a_drive.wallRight ? "wall_stick_right" : "wall_stick_left");
					break;
				case RATCHET_STATE_WALL_JUMP:
					want = a_drive.wallRight ? "wall_jump_right" : "wall_jump_left";
					break;
				case RATCHET_STATE_LEDGE_HANG:
					want = a_drive.ledgeMove == 2 ? "ledge_from_fall" : "ledge_idle";
					break;
				case RATCHET_STATE_LEDGE_CLIMB:
					want = "ledge_jump";
					break;
				default:
					want = "fall";
					break;
				}
			} else if (air && s.airTime > 0.15f) {
				// Skyrim's own fall (off a ledge).
				want = "fall";
			} else if (s.landTimer > 0.0f && a_speed > kWalkToRun) {
				want = "land_run";
			} else if (s.landTimer > 0.0f && a_speed > kMoving) {
				want = "land_walk";
			} else if (a_speed > kMoving && s.walking) {
				want = "walk";
			} else if (a_speed > kMoving) {
				want = "run";
			} else if (s.landTimer > 0.0f) {
				want = "land";
			} else {
				want = "idle";
			}
			const auto landing = [](std::string_view c) { return c.starts_with("land"); };
			const bool quickFade = landing(want) || (s.cur.clip && landing(s.cur.clip->name));
			Play(want, quickFade ? 0.09f : 0.16f);
			s.walking = s.walking ? a_speed < kWalkToRun : a_speed < kRunToWalk;

			// Walks, runs and moving landings keep his feet planted: played at the speed they were
			// authored for at 1x.
			if (s.cur.clip) {
				const auto& n = s.cur.clip->name;
				const float authored = s.cur.clip->AuthoredSpeed();
				if ((n == "walk" || n == "run" || n == "land_walk" || n == "land_run") && authored > 0.1f) {
					const bool land = n.starts_with("land");
					s.cur.rate = std::clamp(a_speed / authored, land ? 0.7f : 0.5f, land ? 1.3f : 1.5f);
				} else {
					s.cur.rate = 1.0f;
				}
			}
		}
	}

	bool Load(const std::filesystem::path& a_pluginDir)
	{
		if (!Settings::Get().ratchetModel) {
			return false;
		}
		s.enabled = s.lib.Load(a_pluginDir / L"RatchetMovement" / L"ratchet.anim");
		return s.enabled;
	}

	bool Enabled() { return s.enabled && !s.failed; }

	std::string_view CurrentClip() { return s.clipName; }

	void SetAttachAllowed(bool a_allowed) { s.attachAllowed = a_allowed; }

	void SetCastShadows(bool a_cast)
	{
		g_castShadows = a_cast;
		ApplyShadows();
	}

	void ShowShapes(int a_only)
	{
		s.showOnly = a_only;
		ApplyShapeVisibility();
	}

	std::vector<std::string> ShapeNames()
	{
		std::vector<std::string> names;
		for (auto* shape : s.shapes) {
			names.emplace_back(shape->name.c_str());
		}
		return names;
	}

	void Update(RE::PlayerCharacter* a_player, float a_dt, const Drive& a_drive)
	{
		if (!Enabled() || !a_player || !s.attachAllowed || !Attach(a_player)) {
			return;
		}
		HideDragonborn(a_player);

		// Where he's going, in the player's frame (the player's root turns with his heading).
		RE::NiPoint3 v{};
		if (auto* ctrl = a_player->GetCharController()) {
			RE::hkVector4 hv;
			ctrl->GetLinearVelocityImpl(hv);
			v = { hv.quad.m128_f32[0], hv.quad.m128_f32[1], hv.quad.m128_f32[2] };
		}
		const float scale = Settings::Get().scale;
		const float speed = std::sqrt(v.x * v.x + v.y * v.y) / scale;  // Ratchet-sized m/s
		const float heading = a_player->GetAngleZ();
		// The world direction he faces: the sim's facing (sim (sin f, cos f) on x, z is east
		// sin f, north -cos f), else where he's going.
		RE::NiPoint2 face{ v.x, v.y };
		bool         turn = speed > kMoving;
		if (a_drive.hasFacing) {
			face = { std::sin(a_drive.facingYaw), -std::cos(a_drive.facingYaw) };
			turn = true;
		}
		const float lx = face.x * std::cos(heading) - face.y * std::sin(heading);
		const float ly = face.x * std::sin(heading) + face.y * std::cos(heading);
		if (turn) {
			const float target = std::atan2(-lx, ly);
			if (a_drive.hasFacing) {
				s.yaw = target;  // the sim already turns him at Rift Apart's rates
			} else {
				const float d = WrapAngle(target - s.yaw);
				const float step = kTurnRate * a_dt;
				s.yaw = WrapAngle(s.yaw + std::clamp(d, -step, step));
			}
		}

		PickClip(a_drive, speed, v.z / scale, a_dt);
		if (!s.cur.clip) {
			return;
		}
		s.cur.time += a_dt * s.cur.rate;
		if (s.prev.clip && s.fade < 1.0f) {
			s.prev.time += a_dt * s.prev.rate;
			s.fade = std::min(1.0f, s.fade + a_dt / s.fadeLen);
			s.lib.Sample(*s.prev.clip, s.prev.time, s.prev.loop, s.pose);
			s.lib.Sample(*s.cur.clip, s.cur.time, s.cur.loop, s.other);
			const float w = s.fade * s.fade * (3.0f - 2.0f * s.fade);
			Anim::Blend(s.pose, s.other, w);
		} else {
			s.lib.Sample(*s.cur.clip, s.cur.time, s.cur.loop, s.pose);
		}

		for (std::size_t i = 0; i < s.nodes.size(); ++i) {
			auto* node = s.nodes[i];
			if (!node) {
				continue;
			}
			node->local.translate = s.pose.t[i];
			node->local.rotate = s.pose.r[i].ToRotation();
			node->local.scale = s.pose.s[i];
		}
		s.model->local.translate = {};
		s.model->local.rotate = Orientation(s.yaw);
		s.model->local.scale = UnitsPerMetre() * scale;
		RE::NiUpdateData update{ 0.0f, RE::NiUpdateData::Flag::kNone };
		s.model->Update(update);
	}
}

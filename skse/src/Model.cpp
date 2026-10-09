// SPDX-License-Identifier: GPL-3.0-or-later
#include "Model.h"

#include "Anim.h"
#include "Clank.h"
#include "Combat.h"
#include "Settings.h"
#include "Sounds.h"

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
		constexpr float kBriefFall = 0.15f;  // seconds: a fall this short isn't shown (Skyrim's own grace)

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
			// His clips as the sandbox picks them (libratchet-anim): what it plays this frame, and
			// where he was (a jump of more than 5 m in a frame, or a new world, is a respawn).
			RatchetAnim*                  anim = nullptr;
			std::vector<RatchetAnimLayer> raw;
			std::vector<Anim::Layer>      layers;
			std::optional<RE::NiPoint3>   lastFeet;
			const void*                   lastWorld = nullptr;
			// The sounds each clip calls for: seconds into it, the moment.
			std::unordered_map<std::string, std::vector<std::pair<float, std::string>>> clipSounds;
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
			if (a_clip.starts_with("strafe_") && !a_clip.starts_with("strafe_jump") && !a_clip.starts_with("strafe_land") &&
				!a_clip.starts_with("strafe_double")) {
				return true;
			}
			return a_clip == "idle" || a_clip == "run" || a_clip == "walk" || a_clip == "fall" || a_clip == "glide" ||
			       a_clip == "wall_slide_left" || a_clip == "wall_slide_right" || a_clip == "ledge_idle" || a_clip == "aim_idle" ||
			       a_clip == "pistol_idle" || a_clip == "pistol_run" || a_clip == "glove_idle";
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

		// His OmniWrench and Burst Pistol (skse\tools\ratchet_to_skyrim.py --name wrench /
		// burstpistol): hung on his gun hand (igLoc_RT_gunAttach, as the sandbox and the game hang
		// them), in the same glTF space as his joints, so with no offset.
		struct Weapon
		{
			const char*               path;
			RE::NiPointer<RE::NiNode> node;
			bool                      failed = false;
		};
		Weapon g_wrench{ "RatchetMovement\\wrench.nif" };
		Weapon g_pistol{ "RatchetMovement\\burstpistol.nif" };

		bool Load(Weapon& a_weapon)
		{
			if (!a_weapon.node && !a_weapon.failed) {
				RE::NiPointer<RE::NiNode> loaded;
				const RE::BSModelDB::DBTraits::ArgsType args{};
				const auto err = RE::BSModelDB::Demand(a_weapon.path, loaded, args);
				if (err != RE::BSResource::ErrorCode::kNone || !loaded) {
					SKSE::log::warn("ratchet model: no meshes\\{} (error {}): he fights without it in his hand", a_weapon.path, static_cast<int>(err));
					a_weapon.failed = true;
					return false;
				}
				a_weapon.node = loaded;
			}
			return a_weapon.node != nullptr;
		}

		void Hang(Weapon& a_weapon, RE::NiNode* a_hand, bool a_show)
		{
			if (!Load(a_weapon)) {
				return;
			}
			if (a_weapon.node->parent != a_hand) {
				if (a_weapon.node->parent) {
					a_weapon.node->parent->DetachChild(a_weapon.node.get());
				}
				a_hand->AttachChild(a_weapon.node.get(), true);
				// Back in his hand's own space with no offset: a throw left its flight's place,
				// turn and size on it, and the caught wrench was drawn a metre or more off his hand.
				a_weapon.node->local = RE::NiTransform{};
			}
			if (a_weapon.node->GetAppCulled() == a_show) {
				a_weapon.node->SetAppCulled(!a_show);
			}
		}

		// Something flying (the thrown wrench, a Shatterbomb) at a_at in the world (game units)
		// turned a_world in it, at his size. In third person under his model, not the player's
		// root: everything else under the root is the Dragonborn's, which HideDragonborn culls
		// every frame (the first build hid the thrown wrench that way). In first person, where his
		// model isn't drawn, under the first-person 3D.
		void PlaceInWorld(RE::NiAVObject* a_node, RE::PlayerCharacter* a_player, const RE::NiPoint3& a_at, const RE::NiMatrix3& a_world)
		{
			auto*       root = a_player->Get3D(false) ? a_player->Get3D(false)->AsNode() : nullptr;
			const auto* camera = RE::PlayerCamera::GetSingleton();
			auto*       first = camera && camera->IsInFirstPerson() && a_player->Get3D(true) ? a_player->Get3D(true)->AsNode() : nullptr;
			RE::NiNode* holder = first ? first : s.model.get();
			if (!holder || (!first && !root)) {
				return;
			}
			if (a_node->parent != holder) {
				if (a_node->parent) {
					a_node->parent->DetachChild(a_node);
				}
				holder->AttachChild(a_node, true);
			}
			// The holder's world this frame (his model's local is set; Skyrim updates its world after).
			const RE::NiTransform parent = first ? first->world : root->world * s.model->local;
			const RE::NiMatrix3   back = parent.rotate.Transpose();
			a_node->local.translate = (back * (a_at - parent.translate)) / parent.scale;
			a_node->local.rotate = back * a_world;
			a_node->local.scale = UnitsPerMetre() * Settings::Get().scale / parent.scale;
			if (a_node->GetAppCulled()) {
				a_node->SetAppCulled(false);
			}
			if (first) {
				// The first-person 3D has had its update this frame; his model's comes after this.
				RE::NiUpdateData update{ 0.0f, RE::NiUpdateData::Flag::kNone };
				a_node->Update(update);
			}
		}

		// The thrown wrench: where the sim has it, laid flat and spun about the up axis.
		void Throw(Weapon& a_weapon, RE::PlayerCharacter* a_player, const RE::NiPoint3& a_at, float a_spin)
		{
			if (!Load(a_weapon) || !s.model) {
				return;
			}
			// glTF to Skyrim (Orientation's M), the handle (glTF +Y, Skyrim up after M) laid along
			// the ground (90° about x), then the spin about up.
			RE::NiMatrix3 flat;  // identity, made a turn of 90° about x
			flat.entry[1][1] = 0.0f;
			flat.entry[1][2] = -1.0f;
			flat.entry[2][1] = 1.0f;
			flat.entry[2][2] = 0.0f;
			RE::NiMatrix3 spin;
			const float c = std::cos(a_spin), sn = std::sin(a_spin);
			spin.entry[0][0] = c;
			spin.entry[0][1] = -sn;
			spin.entry[1][0] = sn;
			spin.entry[1][1] = c;
			spin.entry[2][2] = 1.0f;
			PlaceInWorld(a_weapon.node.get(), a_player, a_at, spin * flat * Orientation(0.0f));
		}

		// The Shatterbombs in the air (skse\tools\ratchet_to_skyrim.py --name shatterbomb): a copy
		// of its model each, tumbling about Skyrim's east-west axis (the sandbox tumbles them about
		// one axis too).
		Weapon                                 g_bomb{ "RatchetMovement\\shatterbomb.nif" };
		std::vector<RE::NiPointer<RE::NiNode>> g_bombs;  // one per bomb in the air; spares hidden

		void ShowBombs(RE::PlayerCharacter* a_player)
		{
			const auto bombs = Combat::Bombs();
			if (bombs.size() > g_bombs.size() && !Load(g_bomb)) {
				return;
			}
			while (g_bombs.size() < bombs.size()) {
				auto* copy = g_bomb.node->Clone();
				auto* node = copy ? copy->AsNode() : nullptr;
				if (!node) {
					SKSE::log::warn("ratchet model: couldn't copy the Shatterbomb's model");
					return;
				}
				g_bombs.emplace_back(node);
			}
			for (std::size_t i = 0; i < g_bombs.size(); ++i) {
				if (i < bombs.size()) {
					const float  c = std::cos(bombs[i].second), sn = std::sin(bombs[i].second);
					RE::NiMatrix3 tumble;  // identity, made a turn about x
					tumble.entry[1][1] = c;
					tumble.entry[1][2] = -sn;
					tumble.entry[2][1] = sn;
					tumble.entry[2][2] = c;
					PlaceInWorld(g_bombs[i].get(), a_player, bombs[i].first, tumble * Orientation(0.0f));
				} else if (!g_bombs[i]->GetAppCulled()) {
					g_bombs[i]->SetAppCulled(true);
				}
			}
		}

		// The Shatterbomb's glove (--name shatterbomb_glove), on in place of his own right glove
		// (sm05) while the Shatterbomb is out, except on a wall, a ledge or a swing (the game
		// holsters it there). Its shapes are skinned to its own copy of his skeleton; as the
		// sandbox does, they're bound to his joints by name instead (his <joint>_scaled nodes where
		// he has them, as his own skin is), so it moves with his hand.
		struct Glove
		{
			RE::NiPointer<RE::NiNode> node;
			bool                      failed = false;
			RE::NiNode*               boundTo = nullptr;  // his model it's bound to
			RE::NiAVObject*           bare = nullptr;     // his own right glove
		};
		Glove g_glove;

		bool BindGlove()
		{
			int bound = 0;
			int missing = 0;
			RE::BSVisit::TraverseScenegraphGeometries(g_glove.node.get(), [&](RE::BSGeometry* a_geom) {
				auto* skin = a_geom->GetGeometryRuntimeData().skinInstance.get();
				if (!skin || !skin->skinData || !skin->bones) {
					return RE::BSVisit::BSVisitControl::kContinue;
				}
				for (std::uint32_t i = 0; i < skin->skinData->GetBoneCount(); ++i) {
					const std::string_view own = skin->bones[i] ? skin->bones[i]->name.c_str() : "";
					if (!own.starts_with("RG_")) {
						continue;
					}
					const std::string joint(own.substr(3));
					RE::NiAVObject*   his = s.model->GetObjectByName(RE::BSFixedString("RC_" + joint + "_scaled"));
					if (!his) {
						his = s.model->GetObjectByName(RE::BSFixedString("RC_" + joint));
					}
					if (!his) {
						++missing;
						continue;
					}
					skin->bones[i] = his;
					skin->boneWorldTransforms[i] = &his->world;
					++bound;
				}
				// Its bound sits where its own skeleton rests; his hand reaches out from there.
				a_geom->GetModelData().modelBound.radius = std::max(a_geom->GetModelData().modelBound.radius, 1.0f);
				return RE::BSVisit::BSVisitControl::kContinue;
			});
			SKSE::log::info("ratchet model: the Shatterbomb's glove bound to {} of his joints ({} not found)", bound, missing);
			return bound > 0;
		}

		void WearGlove(const Drive& a_drive)
		{
			const bool out = Combat::Selected() == Combat::Gun::kShatterbomb;
			if (!out && !g_glove.node) {
				return;
			}
			if (!g_glove.node && !g_glove.failed) {
				RE::NiPointer<RE::NiNode> loaded;
				const RE::BSModelDB::DBTraits::ArgsType args{};
				const auto err = RE::BSModelDB::Demand("RatchetMovement\\shatterbomb_glove.nif", loaded, args);
				if (err != RE::BSResource::ErrorCode::kNone || !loaded) {
					SKSE::log::warn("ratchet model: no meshes\\RatchetMovement\\shatterbomb_glove.nif (error {}): he throws with his own glove",
						static_cast<int>(err));
					g_glove.failed = true;
					return;
				}
				g_glove.node = loaded;
			}
			if (!g_glove.node) {
				return;
			}
			if (g_glove.boundTo != s.model.get()) {
				g_glove.boundTo = s.model.get();
				if (g_glove.node->parent) {
					g_glove.node->parent->DetachChild(g_glove.node.get());
				}
				s.model->AttachChild(g_glove.node.get(), true);
				g_glove.bare = s.model->GetObjectByName(RE::BSFixedString("RC_sm05"));
				if (!BindGlove()) {
					g_glove.failed = true;
					g_glove.node->SetAppCulled(true);
					return;
				}
			}
			const bool on = out && !g_glove.failed && !(a_drive.simState >= RATCHET_STATE_WALL_STICK && a_drive.simState <= RATCHET_STATE_SWING_JUMP);
			if (g_glove.node->GetAppCulled() == on) {
				g_glove.node->SetAppCulled(!on);
			}
			if (g_glove.bare && g_glove.bare->GetAppCulled() != on) {
				g_glove.bare->SetAppCulled(on);
			}
		}

		void HoldWeapons(RE::PlayerCharacter* a_player)
		{
			static RE::NiNode* hand = nullptr;
			static RE::NiNode* handOf = nullptr;
			if (handOf != s.model.get()) {
				handOf = s.model.get();
				auto* found = s.model->GetObjectByName(RE::BSFixedString("RC_igLoc_RT_gunAttach"));
				hand = found ? found->AsNode() : nullptr;
				if (!hand) {
					SKSE::log::warn("ratchet model: no RC_igLoc_RT_gunAttach node: no weapons in his hand");
				}
			}
			if (!hand) {
				return;
			}
			const auto held = Combat::InHand();
			if (const auto thrown = Combat::ThrownWrench()) {
				Throw(g_wrench, a_player, thrown->first, thrown->second);
			} else {
				Hang(g_wrench, hand, held == Combat::Hand::kWrench);
			}
			Hang(g_pistol, hand, held == Combat::Hand::kPistol);
		}

		// Additive layers over the move clip, as the sandbox's graph adds them
		// (apps/sandbox/src/ratchet_model.rs): his upper body bending toward the crosshair while
		// he strafes (the pistol's aim poses scrubbed by the angle up or down and to the side of
		// where he faces), the pistol's recoil on each shot, and the throw.
		struct Layers
		{
			float         aimWeight = 0.0f;
			float         fireTime = -1.0f;  // < 0: not playing
			float         throwTime = -1.0f;
			std::uint32_t shotsSeen = 0;
			std::uint32_t throwsSeen = 0;
			// The Shatterbomb: its wind-up (and which version), its throw, and the glove's hand.
			float         windupWeight = 0.0f;
			float         windupTime = 0.0f;
			int           windupVersion = 0;
			float         bombTime = -1.0f;
			int           bombVersion = 0;
			std::uint32_t bombsSeen = 0;
			float         gloveWeight = 0.0f;
			float         gloveTime = 0.0f;
		};
		Layers g_layers;

		// The game's grenade throws (underhand, additive), whole-body standing still and the arm's
		// on the move or in the air, and their wind-ups while fire is held (the sandbox's
		// GRENADE_LAYERS and WINDUP_LAYERS); the glove's hand pose over whatever he does.
		constexpr std::string_view kBombThrow[] = { "grenade_throw_additive", "grenade_throw_arm_additive" };
		constexpr std::string_view kBombWindup[] = { "grenade_windup_additive", "grenade_windup_arm_additive" };
		constexpr float            kGloveFade = 0.2f;

		constexpr float kAimUp = 90.0f;  // degrees the aim poses reach (the sandbox's)
		constexpr float kAimDown = 45.0f;
		constexpr float kAimSide = 90.0f;
		constexpr float kLayerFadeIn = 0.08f;
		constexpr float kLayerFadeOut = 0.25f;

		void AddLayers(RE::PlayerCharacter* a_player, const Drive& a_drive, float a_speed, float a_dt)
		{
			const auto play = [](std::string_view a_name, float a_t, float a_weight) {
				if (const Anim::Clip* clip = s.lib.Find(a_name)) {
					s.lib.AddAdditive(*clip, a_t, a_weight, s.pose);
				}
			};

			// Aiming (not while dashing: the dash clips are whole-body).
			const bool aiming = a_drive.strafing && a_drive.simState != RATCHET_STATE_DASH && a_drive.simState != RATCHET_STATE_AIR_DASH;
			const float fade = a_dt / kLayerFadeIn;
			g_layers.aimWeight = aiming ? std::min(1.0f, g_layers.aimWeight + fade) : std::max(0.0f, g_layers.aimWeight - fade);
			if (g_layers.aimWeight > 0.0f && a_drive.aimAt) {
				const float        u = UnitsPerMetre();
				const RE::NiPoint3 from = a_player->GetPosition() + RE::NiPoint3{ 0.0f, 0.0f, 0.85f * Settings::Get().scale * u };
				const RE::NiPoint3 d = *a_drive.aimAt - from;
				// The sim's yaw: atan2(x, z) with z south.
				const float yaw = std::atan2(d.x, -d.y) - a_drive.facingYaw;
				const float side = WrapAngle(yaw) * (180.0f / 3.14159265f);
				const float pitch = std::atan2(d.z, std::sqrt(d.x * d.x + d.y * d.y)) * (180.0f / 3.14159265f);
				const bool  running = a_speed > kMoving;
				const char* vertical = running ? "aim_pistol_run_v_additive" : "aim_pistol_v_additive";
				const char* across = running ? "aim_pistol_run_h_additive" : "aim_pistol_h_additive";
				if (const Anim::Clip* clip = s.lib.Find(vertical)) {
					const float half = clip->seconds * 0.5f;
					const float t = pitch >= 0.0f ? half * (1.0f - pitch / kAimUp) : half * (1.0f - pitch / kAimDown);
					s.lib.AddAdditive(*clip, std::clamp(t, 0.0f, clip->seconds * 0.999f), g_layers.aimWeight, s.pose);
				}
				if (const Anim::Clip* clip = s.lib.Find(across)) {
					const float half = clip->seconds * 0.5f;
					const float t = half * (1.0f + side / kAimSide);
					s.lib.AddAdditive(*clip, std::clamp(t, 0.0f, clip->seconds * 0.999f), g_layers.aimWeight, s.pose);
				}
			}

			// The recoil, restarted by each shot.
			if (a_drive.shots != g_layers.shotsSeen) {
				g_layers.shotsSeen = a_drive.shots;
				g_layers.fireTime = 0.0f;
			}
			if (g_layers.fireTime >= 0.0f) {
				g_layers.fireTime += a_dt;
				const Anim::Clip* clip = s.lib.Find("fire_pistol_additive");
				if (!clip || g_layers.fireTime >= clip->seconds) {
					g_layers.fireTime = -1.0f;
				} else {
					const float t = g_layers.fireTime;
					play(clip->name, t, std::min({ t / 0.03f, 1.0f, (clip->seconds - t) / 0.12f }));
				}
			}

			// The Shatterbomb's wind-up while fire is held, and its throw restarted by each one.
			const bool standing = (a_drive.simJump ? a_drive.simState == RATCHET_STATE_GROUND : a_drive.grounded) && a_speed <= kMoving;
			const bool winding = Combat::WindingUp();
			if (winding && g_layers.windupWeight == 0.0f) {
				g_layers.windupTime = 0.0f;
				g_layers.windupVersion = standing ? 0 : 1;
			}
			g_layers.windupWeight = winding ? std::min(1.0f, g_layers.windupWeight + a_dt / kLayerFadeIn) :
			                                  std::max(0.0f, g_layers.windupWeight - a_dt / kLayerFadeOut);
			if (g_layers.windupWeight > 0.0f) {
				g_layers.windupTime += a_dt;
				play(kBombWindup[g_layers.windupVersion], g_layers.windupTime, g_layers.windupWeight);
			}
			if (Combat::BombThrows() != g_layers.bombsSeen) {
				g_layers.bombsSeen = Combat::BombThrows();
				g_layers.bombTime = 0.0f;
				g_layers.bombVersion = standing ? 0 : 1;
			}
			if (g_layers.bombTime >= 0.0f) {
				g_layers.bombTime += a_dt;
				const Anim::Clip* clip = s.lib.Find(kBombThrow[g_layers.bombVersion]);
				if (!clip || g_layers.bombTime >= clip->seconds) {
					g_layers.bombTime = -1.0f;
				} else {
					const float t = g_layers.bombTime;
					play(clip->name, t, std::min({ t / kLayerFadeIn, 1.0f, (clip->seconds - t) / kLayerFadeOut }));
				}
			}
			// The glove's hand while it's on (his fingers round its grip).
			const bool glove = Combat::Selected() == Combat::Gun::kShatterbomb && !(a_drive.simState >= RATCHET_STATE_WALL_STICK &&
																				a_drive.simState <= RATCHET_STATE_SWING_JUMP);
			g_layers.gloveWeight = glove ? std::min(1.0f, g_layers.gloveWeight + a_dt / kGloveFade) : std::max(0.0f, g_layers.gloveWeight - a_dt / kGloveFade);
			if (g_layers.gloveWeight > 0.0f) {
				g_layers.gloveTime += a_dt;
				if (const Anim::Clip* clip = s.lib.Find("glove_hand")) {
					s.lib.Overlay(*clip, g_layers.gloveTime, true, g_layers.gloveWeight, s.pose);
				}
			}

			// The wrench's throw, restarted by each one.
			if (a_drive.throws != g_layers.throwsSeen) {
				g_layers.throwsSeen = a_drive.throws;
				g_layers.throwTime = 0.0f;
			}
			if (g_layers.throwTime >= 0.0f) {
				g_layers.throwTime += a_dt;
				const Anim::Clip* clip = s.lib.Find("throw_additive");
				if (!clip || g_layers.throwTime >= clip->seconds) {
					g_layers.throwTime = -1.0f;
				} else {
					const float t = g_layers.throwTime;
					play(clip->name, t, std::min({ t / kLayerFadeIn, 1.0f, (clip->seconds - t) / kLayerFadeOut }));
				}
			}
		}

		// His clips, as the sandbox's animation graph plays them: libratchet-anim picks them from
		// the sim's state and his weapons, with their crossfades and clocks, and the library blends
		// them as Bevy does (a_velocity: how he really moves, m/s, Skyrim's axes).
		void Animate(RE::PlayerCharacter* a_player, const Drive& a_drive, const RE::NiPoint3& a_velocity, float a_dt)
		{
			const float        u = UnitsPerMetre();
			const auto         weapons = Combat::ForAnimation();
			const RE::NiPoint3 feet = a_player->GetPosition();
			const void*        world = a_player->GetWorldspace() ? static_cast<const void*>(a_player->GetWorldspace()) :
			                                                       static_cast<const void*>(a_player->GetParentCell());
			const bool         respawned = s.lastFeet && (world != s.lastWorld || feet.GetDistance(*s.lastFeet) > 5.0f * u);
			s.lastFeet = feet;
			s.lastWorld = world;
			const auto sim = [u](const RE::NiPoint3& a_p) { return RatchetVec3{ a_p.x / u, a_p.z / u, -a_p.y / u }; };

			RatchetAnimInput in{};
			in.dt = a_dt;
			in.scale = Settings::Get().scale;
			in.velocity = { a_velocity.x, a_velocity.z, -a_velocity.y };
			in.feet = sim(feet);
			in.has_ground_distance = a_drive.groundDistance ? 1 : 0;
			in.ground_distance = a_drive.groundDistance.value_or(0.0f);
			in.events = a_drive.events;
			in.respawned = respawned ? 1 : 0;
			in.slot = static_cast<std::uint8_t>(weapons.shatterbomb ? RATCHET_SLOT_SHATTERBOMB : RATCHET_SLOT_BURST_PISTOL);
			in.aiming = weapons.aiming ? 1 : 0;
			in.trigger = weapons.trigger ? 1 : 0;
			in.stance = weapons.stance;
			in.shots = weapons.shots;
			in.grenade_throws = weapons.grenadeThrows;
			in.cooking = weapons.cooking ? 1 : 0;
			in.equips = weapons.equips;
			in.aim_target = sim(weapons.aimAt);
			in.wrench_throwing = weapons.wrenchOut ? 1 : 0;
			in.wrench_throws = weapons.wrenchThrows;
			in.wrench_catches = weapons.wrenchCatches;

			const auto& clips = s.lib.Clips();
			s.raw.resize(clips.size());
			const std::uint32_t n = ratchet_anim_update(s.anim, a_drive.sim, &in, s.raw.data(), static_cast<std::uint32_t>(s.raw.size()));
			s.layers.clear();
			for (std::uint32_t i = 0; i < n; ++i) {
				const auto& l = s.raw[i];
				if (l.clip >= clips.size()) {
					continue;
				}
				const auto kind = l.kind == RATCHET_LAYER_ADDITIVE        ? Anim::Layer::Kind::kAdditive :
				                  l.kind == RATCHET_LAYER_BASE_UPPER_BODY ? Anim::Layer::Kind::kBaseUpperBody :
				                                                            Anim::Layer::Kind::kBase;
				s.layers.push_back({ &clips[l.clip], l.time, l.weight, kind });
			}
			s.lib.Mix(s.layers, s.pose);
			const std::string_view picked = ratchet_anim_clip_name(s.anim);
			if (picked != s.clipName && Settings::Get().traceJumps) {
				SKSE::log::info("clip: {} ({} layers)", picked, s.layers.size());
			}
			s.clipName = picked;

			// The sounds his clip calls for, on its clock (a frame after the pose, as the sandbox's).
			RatchetAnimClock clock{};
			if (!s.clipSounds.empty() && ratchet_anim_clock(s.anim, &clock) && clock.clip < clips.size()) {
				if (const auto it = s.clipSounds.find(clips[clock.clip].name); it != s.clipSounds.end()) {
					const RE::NiPoint3 at = feet + RE::NiPoint3{ 0.0f, 0.0f, 0.5f * Settings::Get().scale * u };
					for (const auto& [t, moment] : it->second) {
						if (ratchet_anim_sound_due(s.anim, t)) {
							Sounds::Play(moment, at);
						}
					}
				}
			}
		}

		void PickClip(const Drive& a_drive, float a_speed, float a_vy, float a_dt)
		{
			const bool air = a_drive.simJump ? a_drive.simState != RATCHET_STATE_GROUND : !a_drive.grounded;
			s.airTime = air ? s.airTime + a_dt : 0.0f;
			const bool landed = !air && s.wasAir;
			// A fall shorter than kBriefFall (the sim's, off a bump or a step) shows neither the
			// fall nor a landing, as Skyrim's own short falls don't.
			const bool fall = a_drive.simJump && a_drive.simState == RATCHET_STATE_FALL;
			s.wasAir = air && ((a_drive.simJump && !fall) || s.airTime > kBriefFall);
			if (landed) {
				s.landTimer = a_speed > kMoving ? 0.35f : 0.3f;
			}
			s.landTimer = std::max(0.0f, s.landTimer - a_dt);

			std::string_view want;
			if (fall && s.airTime <= kBriefFall && s.cur.clip) {
				want = s.cur.clip->name;  // what he was doing goes on for now
			} else if (a_drive.simJump && a_drive.simState != RATCHET_STATE_GROUND) {
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
				// The wrench: the combo's three swings, the Hyper-Strike's hop and slam.
				case RATCHET_STATE_MELEE:
					{
						static constexpr std::string_view kSwings[] = { "melee_1", "melee_2", "melee_3" };
						want = kSwings[std::min<std::uint32_t>(a_drive.meleeSwing, 2)];
					}
					break;
				case RATCHET_STATE_SMASH:
					want = "smash_intro";
					break;
				case RATCHET_STATE_SMASH_LAND:
					want = "smash";
					break;
				default:
					want = "fall";
					break;
				}
			} else if (air && s.airTime > kBriefFall) {
				// Skyrim's own fall (off a ledge).
				want = "fall";
			} else if (a_drive.strafing) {
				// Aiming or firing: he faces the aim and strafes, the clip by the way he moves
				// (n ahead, e to his right).
				static constexpr std::string_view kStrafe[] = { "strafe_n", "strafe_ne", "strafe_e", "strafe_se", "strafe_s", "strafe_sw", "strafe_w", "strafe_nw" };
				if (a_speed > kMoving) {
					const int sector = static_cast<int>(std::floor(a_drive.strafeAngle / (3.14159265f / 4.0f) + 0.5f));
					want = kStrafe[((sector % 8) + 8) % 8];
				} else {
					want = "aim_idle";
				}
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
				// With the Shatterbomb out, his idle holds the glove up.
				want = Combat::Selected() == Combat::Gun::kShatterbomb ? "glove_idle" : "idle";
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
				const bool strafeRun = n.starts_with("strafe_") && !n.starts_with("strafe_jump") && !n.starts_with("strafe_land") &&
				                       !n.starts_with("strafe_double");
				if ((n == "walk" || n == "run" || n == "land_walk" || n == "land_run" || strafeRun) && authored > 0.1f) {
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
		if (s.enabled) {
			// The sandbox's clip choice, given his clips (no JumpPeak events in ratchet.anim: the
			// swing jump plays at 1x).
			s.lib.SetUpperBody("RC_spine_1");
			std::vector<RatchetClipInfo> infos;
			for (const auto& clip : s.lib.Clips()) {
				infos.push_back({ clip.name.c_str(), clip.seconds, clip.rootDistance, -1.0f });
			}
			s.anim = ratchet_anim_new(infos.data(), static_cast<std::uint32_t>(infos.size()));
			SKSE::log::info("ratchet model: {}", s.anim ? std::format("the sandbox's clip choice over {} clips", infos.size()) :
			                                               "no animator from ratchet.dll: the plugin's own clip choice");
			// skse\tools\ratchet_sounds_to_skyrim.py: "<clip> <seconds> <moment>" lines.
			std::ifstream file(a_pluginDir / L"RatchetMovement" / L"clip_sounds.txt");
			std::string   clip, moment;
			float         t = 0.0f;
			std::size_t   count = 0;
			while (file >> clip >> t >> moment) {
				s.clipSounds[clip].emplace_back(t, moment);
				++count;
			}
			if (s.anim && Settings::Get().sounds) {
				SKSE::log::info("ratchet model: {} clip sounds in {} clips", count, s.clipSounds.size());
			}
		}
		return s.enabled;
	}

	void Reset()
	{
		if (s.anim) {
			ratchet_anim_reset(s.anim);
		}
		s.lastFeet.reset();
	}

	bool ClipSounds() { return Enabled() && s.anim && !s.clipSounds.empty() && Settings::Get().sounds; }

	bool Enabled() { return s.enabled && !s.failed; }

	std::string_view CurrentClip() { return s.clipName; }

	std::optional<RE::NiPoint3> GunHand()
	{
		if (!s.model || !s.model->parent) {
			return std::nullopt;
		}
		static RE::NiAVObject* hand = nullptr;
		static RE::NiNode*     handOf = nullptr;
		if (handOf != s.model.get()) {
			handOf = s.model.get();
			hand = s.model->GetObjectByName(RE::BSFixedString("RC_igLoc_RT_gunAttach"));
			if (!hand) {
				hand = s.model->GetObjectByName(RE::BSFixedString("RC_RT_handle_bound"));
			}
		}
		if (!hand) {
			return std::nullopt;
		}
		return hand->world.translate;
	}

	std::optional<RE::NiTransform> PistolLocator(std::string_view a_name)
	{
		auto* gun = g_pistol.node.get();
		if (!gun || !gun->parent || gun->GetAppCulled()) {
			return std::nullopt;
		}
		// ratchet_to_skyrim.py --name burstpistol prefixes its nodes RP_.
		auto* found = gun->GetObjectByName(RE::BSFixedString(std::format("RP_{}", a_name)));
		if (!found) {
			found = gun->GetObjectByName(RE::BSFixedString(std::string(a_name)));
		}
		if (!found) {
			static bool logged = false;
			if (!logged) {
				logged = true;
				SKSE::log::warn("ratchet model: the Burst Pistol has no {} node", a_name);
			}
			return std::nullopt;
		}
		return found->world;
	}

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

		if (s.anim && a_drive.sim) {
			Animate(a_player, a_drive, v, a_dt);
		} else {
			PickClip(a_drive, speed, v.z / scale, a_dt);
			if (!s.cur.clip) {
				return;
			}
			s.cur.time += a_dt * s.cur.rate;
			// A looping clip's time wraps (hours of idling would run its float precision out).
			if (s.cur.loop && s.cur.clip->seconds > 0.0f) {
				s.cur.time = std::fmod(s.cur.time, s.cur.clip->seconds);
			}
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
			AddLayers(a_player, a_drive, speed, a_dt);
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
		HoldWeapons(a_player);
		WearGlove(a_drive);
		ShowBombs(a_player);
		Clank::Update(s.model.get(), a_drive.simJump && a_drive.simState == RATCHET_STATE_GLIDE, a_dt);
		RE::NiUpdateData update{ 0.0f, RE::NiUpdateData::Flag::kNone };
		s.model->Update(update);
		// Once a throw, a few frames out: where the flying wrench is drawn against where it flies.
		static int thrownFrames = 0;
		if (const auto thrown = Combat::ThrownWrench(); thrown && g_wrench.node) {
			if (++thrownFrames == 10) {
				SKSE::log::info("ratchet model: the thrown wrench is drawn {:.2f} m from where it flies ({}, under {})",
					g_wrench.node->world.translate.GetDistance(thrown->first) / UnitsPerMetre(), g_wrench.node->GetAppCulled() ? "culled" : "shown",
					g_wrench.node->parent ? g_wrench.node->parent->name.c_str() : "nothing");
			}
		} else {
			thrownFrames = 0;
		}
	}
}

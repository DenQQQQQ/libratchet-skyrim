// SPDX-License-Identifier: GPL-3.0-or-later
#include "Clank.h"

#include "Anim.h"
#include "Settings.h"

namespace Clank
{
	namespace
	{
		constexpr const char* kModelPath = "RatchetMovement\\clank.nif";  // under Data\meshes
		constexpr const char* kRatchetBone = "RC_ClankAttach";
		constexpr std::string_view kAttachJoint = "RK_dyn_backpack_attach";
		// His heli-pack's shapes: the converter puts their part in their names.
		constexpr std::string_view kHeliShapes = "RK_heli_";
		// Rotor heads (top and both forearms): the game spins them in code, not in the clips.
		constexpr std::string_view kRotors[] = { "RK_topProp", "RK_LF_Prop", "RK_RT_Prop" };
		constexpr float kRotorSpeed = 22.0f;  // rad/s
		constexpr float kFade = 0.1f;         // seconds between clips (the sandbox's)
		// The heli-pack's one-shot clips: it comes out and folds away.
		constexpr float kExtend = 0.23f;
		constexpr float kRetract = 0.3f;

		struct Rotor
		{
			std::size_t  joint;
			RE::NiPoint3 axis;  // its spin axis, in the joint's own space
		};

		struct State
		{
			Anim::Library                lib;
			bool                         enabled = false;
			bool                         failed = false;
			RE::NiPointer<RE::NiNode>    model;
			std::vector<RE::NiAVObject*> nodes;  // by joint
			std::vector<RE::BSGeometry*> heli;
			bool                         heliShown = true;  // so the first frame hides them
			std::optional<std::size_t>   attach;
			std::vector<Rotor>           rotors;
			float                        rotorAngle = 0.0f;
			Anim::Player                 player;  // his clips as Bevy plays them (the sandbox's AnimationTransitions)
			std::vector<Anim::Layer>     layers;
			float                        oneShot = 0.0f;  // seconds left of a one-shot clip
			bool                         gliding = false;
			Anim::Pose                   pose;
			RE::NiNode*                  bone = nullptr;  // Ratchet's ClankAttach
			RE::NiNode*                  boneOf = nullptr;
			int                          frames = 0;      // since he went on
		};
		State s;

		std::optional<std::size_t> Joint(std::string_view a_name)
		{
			const auto& joints = s.lib.Joints();
			for (std::size_t i = 0; i < joints.size(); ++i) {
				if (joints[i].name == a_name) {
					return i;
				}
			}
			return std::nullopt;
		}

		// a_joint's turn in a_ancestor's space (a_ancestor's own turn not in it).
		std::optional<RE::NiQuaternion> TurnIn(const Anim::Pose& a_pose, std::size_t a_joint, std::size_t a_ancestor)
		{
			RE::NiQuaternion q{ 1.0f, 0.0f, 0.0f, 0.0f };
			for (auto j = static_cast<std::int32_t>(a_joint); j != static_cast<std::int32_t>(a_ancestor); j = s.lib.Joints()[j].parent) {
				if (j < 0) {
					return std::nullopt;
				}
				q = Anim::Multiply(a_pose.r[j], q);
			}
			return q;
		}

		// a_joint in his model's space (the NIF's root).
		RE::NiTransform InModel(const Anim::Pose& a_pose, std::size_t a_joint)
		{
			RE::NiTransform t;
			for (auto j = static_cast<std::int32_t>(a_joint); j >= 0; j = s.lib.Joints()[j].parent) {
				RE::NiTransform local;
				local.translate = a_pose.t[j];
				local.rotate = a_pose.r[j].ToRotation();
				local.scale = a_pose.s[j];
				t = local * t;
			}
			return t;
		}

		// Each rotor's spin axis: blades A and B sit 120 degrees apart round it, so it's the axis
		// of the turn between their rest frames (in the spin joint's space).
		void FindRotors()
		{
			const Anim::Pose rest = s.lib.Rest();
			for (const auto prefix : kRotors) {
				const std::string p(prefix);
				const auto spin = Joint(p + "BladeSpin");
				const auto a = Joint(p + "BladeA");
				const auto b = Joint(p + "BladeB");
				if (!spin || !a || !b) {
					continue;
				}
				const auto qa = TurnIn(rest, *a, *spin);
				const auto qb = TurnIn(rest, *b, *spin);
				if (!qa || !qb) {
					continue;
				}
				const RE::NiQuaternion inverseA{ qa->w, -qa->x, -qa->y, -qa->z };
				const RE::NiQuaternion d = Anim::Multiply(*qb, inverseA);
				const float angle = 2.0f * std::acos(std::clamp(d.w, -1.0f, 1.0f));
				const float sine = std::sqrt(std::max(0.0f, 1.0f - d.w * d.w));
				if (std::abs(angle) > 0.1f && sine > 1e-4f) {
					s.rotors.push_back({ *spin, RE::NiPoint3{ d.x, d.y, d.z } / sine });
				}
			}
		}

		bool LoadModel()
		{
			if (s.model) {
				return true;
			}
			if (s.failed) {
				return false;
			}
			RE::NiPointer<RE::NiNode> loaded;
			const RE::BSModelDB::DBTraits::ArgsType args{};
			const auto err = RE::BSModelDB::Demand(kModelPath, loaded, args);
			if (err != RE::BSResource::ErrorCode::kNone || !loaded) {
				SKSE::log::warn("clank: no meshes\\{} (error {}): Ratchet goes without him", kModelPath, static_cast<int>(err));
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
			RE::BSVisit::TraverseScenegraphGeometries(s.model.get(), [](RE::BSGeometry* a_geom) {
				if (std::string_view(a_geom->name.c_str()).starts_with(kHeliShapes)) {
					s.heli.push_back(a_geom);
				}
				return RE::BSVisit::BSVisitControl::kContinue;
			});
			s.attach = Joint(kAttachJoint);
			FindRotors();
			s.pose = s.lib.Rest();
			SKSE::log::info("clank: loaded meshes\\{}, {} of {} joints found, {} heli-pack shapes, {} rotors{}", kModelPath, found,
				s.lib.Joints().size(), s.heli.size(), s.rotors.size(), s.attach ? "" : ", no attach joint (he hangs by his root)");
			return true;
		}

		// The sandbox's choice: the heli-pack comes out as the glide starts and folds away as
		// it ends, else his backpack idle.
		void PickClip(bool a_gliding, float a_dt)
		{
			s.oneShot = std::max(0.0f, s.oneShot - a_dt);
			const Anim::Clip* current = s.player.Current();
			std::string_view  want = current ? std::string_view(current->name) : "idle";
			if (a_gliding && !s.gliding) {
				want = "heli_extend";
				s.oneShot = kExtend;
			} else if (!a_gliding && s.gliding) {
				want = "heli_retract";
				s.oneShot = kRetract;
			} else if (s.oneShot <= 0.0f) {
				want = a_gliding ? "glide" : "idle";
			}
			s.gliding = a_gliding;
			const Anim::Clip* clip = s.lib.Find(want);
			if (!clip) {
				clip = s.lib.Find(a_gliding ? "glide" : "idle");
			}
			if (!clip || clip == current) {
				return;
			}
			s.player.Play(clip, current ? kFade : 0.0f, clip->name == "idle" || clip->name == "glide");
		}

		void ShowHeli(bool a_show)
		{
			if (a_show == s.heliShown) {
				return;
			}
			s.heliShown = a_show;
			for (auto* shape : s.heli) {
				shape->SetAppCulled(!a_show);
			}
		}

		// Once, a moment after he's on: where his head is from Ratchet's chest, in Ratchet's
		// model space (metres, +z ahead of Ratchet, +y up), and which way he faces (-1: backward,
		// his back on Ratchet's, as in the game).
		void LogPlace(RE::NiNode* a_ratchet)
		{
			auto* chest = a_ratchet->GetObjectByName(RE::BSFixedString("RC_chest"));
			auto* head = s.model->GetObjectByName(RE::BSFixedString("RK_head"));
			auto* body = s.model->GetObjectByName(RE::BSFixedString("RK_a_body"));
			if (!chest || !head || !body) {
				return;
			}
			const RE::NiTransform& r = a_ratchet->world;
			const RE::NiPoint3 at = (r.rotate.Transpose() * (head->world.translate - chest->world.translate)) / r.scale;
			const RE::NiPoint3 ahead{ 0.0f, 0.0f, 1.0f };  // glTF's forward, for both of them
			const float facing = (body->world.rotate * ahead).Dot(r.rotate * ahead);
			SKSE::log::info("clank: his head {:.2f} m {} Ratchet's chest, {:.2f} m {}, {:.2f} m to the side; facing {:+.2f}",
				std::abs(at.z), at.z < 0.0f ? "behind" : "ahead of", std::abs(at.y), at.y < 0.0f ? "below" : "above", at.x, facing);
		}
	}

	bool Load(const std::filesystem::path& a_pluginDir)
	{
		if (!Settings::Get().clank) {
			return false;
		}
		s.enabled = s.lib.Load(a_pluginDir / L"RatchetMovement" / L"clank.anim");
		return s.enabled;
	}

	bool Enabled() { return s.enabled && !s.failed; }

	std::string_view CurrentClip() { return s.player.Current() ? std::string_view(s.player.Current()->name) : std::string_view{}; }

	bool HeliOut() { return Enabled() && s.model && s.heliShown; }

	void Update(RE::NiNode* a_ratchet, bool a_gliding, float a_dt)
	{
		if (!Enabled() || !a_ratchet) {
			return;
		}
		if (s.boneOf != a_ratchet) {
			s.boneOf = a_ratchet;
			auto* found = a_ratchet->GetObjectByName(RE::BSFixedString(kRatchetBone));
			s.bone = found ? found->AsNode() : nullptr;
			if (!s.bone) {
				SKSE::log::warn("clank: Ratchet's model has no {} node: Ratchet goes without him", kRatchetBone);
			}
		}
		if (!s.bone || !LoadModel()) {
			return;
		}
		if (s.model->parent != s.bone) {
			if (s.model->parent) {
				s.model->parent->DetachChild(s.model.get());
			}
			s.bone->AttachChild(s.model.get(), true);
			SKSE::log::info("clank: on Ratchet's back");
			s.frames = 0;
		}
		if (++s.frames == 30) {
			LogPlace(a_ratchet);
		}

		PickClip(a_gliding, a_dt);
		if (!s.player.Current()) {
			return;
		}
		// His clips mixed as Bevy mixes them (a channel no clip keys keeps last frame's value).
		s.player.Advance(a_dt);
		s.layers.clear();
		s.player.Layers(s.layers);
		s.lib.Mix(s.layers, s.pose);

		// The heli-pack's looks while it's out (extend, glide, retract), its rotors spinning on
		// top of the clip (on the nodes only: in the pose a spin would build up frame on frame).
		const std::string_view clip = s.player.Current()->name;
		ShowHeli(clip == "heli_extend" || clip == "glide" || clip == "heli_retract");
		std::vector<std::pair<std::size_t, RE::NiQuaternion>> spins;
		if (s.heliShown) {
			s.rotorAngle = std::fmod(s.rotorAngle + kRotorSpeed * a_dt, 2.0f * 3.14159265f);
			const float half = 0.5f * s.rotorAngle;
			for (const auto& rotor : s.rotors) {
				const RE::NiPoint3 a = rotor.axis * std::sin(half);
				spins.emplace_back(rotor.joint, RE::NiQuaternion{ std::cos(half), a.x, a.y, a.z });
			}
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
		for (const auto& [joint, spin] : spins) {
			if (auto* node = s.nodes[joint]) {
				node->local.rotate = Anim::Multiply(s.pose.r[joint], spin).ToRotation();
			}
		}
		// His attach joint exactly on Ratchet's bone: his root is the inverse of where the joint
		// is in his model. Its place and turn only, not its scale: his backpack clips scale his
		// a_body to 0.84 (smaller on Ratchet's back), and undoing that made him 1.19 times too big
		// in the sandbox.
		if (s.attach) {
			const RE::NiTransform a = InModel(s.pose, *s.attach);
			const RE::NiMatrix3   back = a.rotate.Transpose();
			s.model->local.rotate = back;
			s.model->local.translate = -(back * a.translate);
			s.model->local.scale = 1.0f;
		}
	}
}

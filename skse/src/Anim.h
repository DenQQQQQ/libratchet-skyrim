// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Ratchet's clips from ratchet.anim (skse/tools/ratchet_to_skyrim.py): his skeleton's rest pose
// and every move clip at its keys, sampled and blended here.
namespace Anim
{
	struct Joint
	{
		std::string   name;
		std::int32_t  parent;
		RE::NiPoint3  t;
		RE::NiQuaternion r;
		float         s;
	};

	struct Track
	{
		std::uint16_t      joint;
		std::uint8_t       path;      // 0 translation, 1 rotation, 2 scale
		bool               constant;
		std::vector<float> values;    // 3, 4 (x y z w) or 1 per frame
	};

	struct Clip
	{
		std::string        name;
		float              seconds = 0.0f;
		float              rate = 30.0f;
		std::uint32_t      frames = 0;
		float              rootDistance = 0.0f;  // metres its root motion travels (Ratchet-sized)
		bool               additive = false;
		std::vector<Track> tracks;

		// Ground speed it was authored for, m/s (0 if it doesn't travel).
		float AuthoredSpeed() const { return seconds > 0.0f ? rootDistance / seconds : 0.0f; }
	};

	// A local transform for every joint.
	struct Pose
	{
		std::vector<RE::NiPoint3>     t;
		std::vector<RE::NiQuaternion> r;
		std::vector<float>            s;
	};

	// A clip to blend, as Bevy's animation graph blends them (the sandbox's: libratchet-anim gives
	// Ratchet's list, Player gives Clank's).
	struct Layer
	{
		enum class Kind : std::uint8_t
		{
			kBase,           // blended with the other base layers by weight
			kBaseUpperBody,  // the same, on the upper body only (Library::SetUpperBody)
			kAdditive,       // its keys are changes, added on top
		};
		const Clip* clip = nullptr;
		float       time = 0.0f;  // seconds into it (wrapped or held by whoever plays it)
		float       weight = 0.0f;
		Kind        kind = Kind::kBase;
	};

	class Library
	{
	public:
		bool Load(const std::filesystem::path& a_file);
		const std::vector<Joint>& Joints() const { return _joints; }
		const Clip* Find(std::string_view a_name) const;

		Pose Rest() const;
		// The clip at time a_t (looped or held at its ends) over the rest pose.
		void Sample(const Clip& a_clip, float a_t, bool a_loop, Pose& a_out) const;
		// An additive clip (its keys are changes) at a_t on top of a_pose, at a_weight, as
		// Bevy's additive blend does it: translations added, rotations turned by
		// slerp(identity, change, weight) after the pose's.
		void AddAdditive(const Clip& a_clip, float a_t, float a_weight, Pose& a_pose) const;

		// The joint a_name and those below it: the upper body, for kBaseUpperBody layers.
		void SetUpperBody(std::string_view a_name);
		// The pose the layers make, as Bevy makes it. Per joint and channel, the base layers that
		// key it, in order: the first sets it, each next one is interpolated toward by its weight
		// over the weights so far (so weights needn't add up to 1: the glove's hand at 20 is 20/21
		// of its joints). A channel no base layer keys keeps a_pose's (last frame's). Then the
		// additive layers: translation += weight × change, rotation = slerp(I, change, weight) ×
		// rotation. Keys are sampled at each layer's time, clamped to the clip, rotations slerped.
		void Mix(std::span<const Layer> a_layers, Pose& a_pose) const;
		// A partial clip (it keys only some joints) at a_t over a_pose, at a_weight: the joints it
		// keys go that far toward its pose, the rest stay (the glove's hand on whatever he does).
		void Overlay(const Clip& a_clip, float a_t, bool a_loop, float a_weight, Pose& a_pose) const;

	private:
		std::vector<Joint> _joints;
		std::vector<Clip>  _clips;
		std::vector<bool>  _upper;  // by joint
	};

	// Bevy's AnimationTransitions with its clips' clocks, for a host that picks its clips itself
	// (Clank, as the sandbox's clank.rs plays him). Play starts a clip from 0 and fades the one
	// playing out over a_fade (0 cuts); each Advance takes the weights first (the newest fading
	// clip keeps its weight × what's left, the clip playing gets the rest), then the clocks (a
	// one-shot holds at its end, finished), and drops the faded ones.
	class Player
	{
	public:
		void Play(const Clip* a_clip, float a_fade, bool a_loop);
		void Advance(float a_dt);
		// Base layers, oldest first, the clip playing last (its weight is what the fades leave).
		void Layers(std::vector<Layer>& a_out) const;
		const Clip* Current() const { return _main.clip; }
		bool Finished() const { return _main.clip && !_main.loop && _main.time >= _main.clip->seconds; }
		void Reset() { *this = Player{}; }

	private:
		struct Playing
		{
			const Clip* clip = nullptr;
			float       time = 0.0f;
			bool        loop = true;
			float       weight = 1.0f;   // its own (a fading clip's declines)
			float       decline = 0.0f;  // per second, while fading out
			float       share = 1.0f;    // what it blends with this frame
		};
		Playing              _main;
		std::vector<Playing> _fading;  // oldest first
	};

	// a = a + (b - a) * w, joint by joint (rotations by shortest-arc nlerp).
	void Blend(Pose& a_a, const Pose& a_b, float a_w);

	// The Hamilton product a * b (b's turn first, then a's), as glam's.
	RE::NiQuaternion Multiply(const RE::NiQuaternion& a_a, const RE::NiQuaternion& a_b);
}

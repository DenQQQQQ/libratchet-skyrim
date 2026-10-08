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

	private:
		std::vector<Joint> _joints;
		std::vector<Clip>  _clips;
	};

	// a = a + (b - a) * w, joint by joint (rotations by shortest-arc nlerp).
	void Blend(Pose& a_a, const Pose& a_b, float a_w);

	// The Hamilton product a * b (b's turn first, then a's), as glam's.
	RE::NiQuaternion Multiply(const RE::NiQuaternion& a_a, const RE::NiQuaternion& a_b);
}

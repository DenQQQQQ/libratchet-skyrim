// SPDX-License-Identifier: GPL-3.0-or-later
#include "Anim.h"

namespace Anim
{
	namespace
	{
		struct In
		{
			const std::vector<char>& data;
			std::size_t              pos = 0;

			template <class T>
			T Get()
			{
				if (pos + sizeof(T) > data.size()) {
					throw std::runtime_error("ratchet.anim is cut short");
				}
				T v;
				std::memcpy(&v, data.data() + pos, sizeof(T));
				pos += sizeof(T);
				return v;
			}

			std::string Str()
			{
				const auto n = Get<std::uint16_t>();
				if (pos + n > data.size()) {
					throw std::runtime_error("ratchet.anim is cut short");
				}
				std::string s(data.data() + pos, n);
				pos += n;
				return s;
			}

			void Floats(std::vector<float>& a_out, std::size_t a_n)
			{
				if (pos + a_n * 4 > data.size()) {
					throw std::runtime_error("ratchet.anim is cut short");
				}
				a_out.resize(a_n);
				std::memcpy(a_out.data(), data.data() + pos, a_n * 4);
				pos += a_n * 4;
			}
		};

		RE::NiQuaternion Normalized(RE::NiQuaternion q)
		{
			const float n = std::sqrt(q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z);
			if (n > 1e-8f) {
				q.w /= n;
				q.x /= n;
				q.y /= n;
				q.z /= n;
			}
			return q;
		}

		RE::NiQuaternion Nlerp(const RE::NiQuaternion& a_a, RE::NiQuaternion a_b, float a_w)
		{
			if (a_a.Dot(a_b) < 0.0f) {
				a_b.Neg();
			}
			return Normalized({ a_a.w + (a_b.w - a_a.w) * a_w, a_a.x + (a_b.x - a_a.x) * a_w, a_a.y + (a_b.y - a_a.y) * a_w,
				a_a.z + (a_b.z - a_a.z) * a_w });
		}

		// The Hamilton product a * b (b's turn first, then a's), as glam's.
		RE::NiQuaternion Multiply(const RE::NiQuaternion& a_a, const RE::NiQuaternion& a_b)
		{
			return Normalized({ a_a.w * a_b.w - a_a.x * a_b.x - a_a.y * a_b.y - a_a.z * a_b.z,
				a_a.w * a_b.x + a_a.x * a_b.w + a_a.y * a_b.z - a_a.z * a_b.y,
				a_a.w * a_b.y - a_a.x * a_b.z + a_a.y * a_b.w + a_a.z * a_b.x,
				a_a.w * a_b.z + a_a.x * a_b.y - a_a.y * a_b.x + a_a.z * a_b.w });
		}
	}

	bool Library::Load(const std::filesystem::path& a_file)
	{
		std::ifstream f(a_file, std::ios::binary);
		if (!f) {
			SKSE::log::warn("ratchet model: no {}", a_file.string());
			return false;
		}
		std::vector<char> data((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
		try {
			In in{ data };
			if (in.Get<std::uint32_t>() != 0x4D4E4152 /* "RANM" */ || in.Get<std::uint32_t>() != 1) {
				SKSE::log::error("ratchet model: {} isn't a version 1 clip file", a_file.string());
				return false;
			}
			_joints.resize(in.Get<std::uint32_t>());
			for (auto& j : _joints) {
				j.name = in.Str();
				j.parent = in.Get<std::int32_t>();
				j.t = { in.Get<float>(), in.Get<float>(), in.Get<float>() };
				const float x = in.Get<float>(), y = in.Get<float>(), z = in.Get<float>(), w = in.Get<float>();
				j.r = { w, x, y, z };
				j.s = in.Get<float>();
			}
			_clips.resize(in.Get<std::uint32_t>());
			for (auto& c : _clips) {
				c.name = in.Str();
				c.seconds = in.Get<float>();
				c.rate = in.Get<float>();
				c.frames = in.Get<std::uint32_t>();
				c.rootDistance = in.Get<float>();
				c.additive = in.Get<std::uint8_t>() != 0;
				c.tracks.resize(in.Get<std::uint32_t>());
				for (auto& t : c.tracks) {
					t.joint = in.Get<std::uint16_t>();
					t.path = in.Get<std::uint8_t>();
					t.constant = in.Get<std::uint8_t>() != 0;
					const std::size_t width = t.path == 0 ? 3 : t.path == 1 ? 4 : 1;
					in.Floats(t.values, width * (t.constant ? 1 : c.frames));
					if (t.joint >= _joints.size()) {
						throw std::runtime_error("a track names a joint that isn't there");
					}
				}
			}
		} catch (const std::exception& e) {
			SKSE::log::error("ratchet model: {}: {}", a_file.string(), e.what());
			_joints.clear();
			_clips.clear();
			return false;
		}
		SKSE::log::info("ratchet model: {} joints, {} clips from {}", _joints.size(), _clips.size(), a_file.string());
		return true;
	}

	const Clip* Library::Find(std::string_view a_name) const
	{
		for (const auto& c : _clips) {
			if (c.name == a_name) {
				return &c;
			}
		}
		return nullptr;
	}

	Pose Library::Rest() const
	{
		Pose p;
		p.t.reserve(_joints.size());
		p.r.reserve(_joints.size());
		p.s.reserve(_joints.size());
		for (const auto& j : _joints) {
			p.t.push_back(j.t);
			p.r.push_back(j.r);
			p.s.push_back(j.s);
		}
		return p;
	}

	void Library::Sample(const Clip& a_clip, float a_t, bool a_loop, Pose& a_out) const
	{
		// Joints the clip doesn't key stay at rest (not where the last clip sampled into this pose
		// left them).
		a_out = Rest();
		const float last = static_cast<float>(a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		float f = a_t * a_clip.rate;
		if (a_loop && last > 0.0f) {
			f = std::fmod(f, last);
			if (f < 0.0f) {
				f += last;
			}
		} else {
			f = std::clamp(f, 0.0f, last);
		}
		const auto i0 = static_cast<std::uint32_t>(f);
		const auto i1 = std::min(i0 + 1, a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		const float w = f - static_cast<float>(i0);
		for (const auto& tr : a_clip.tracks) {
			const float* v = tr.values.data();
			switch (tr.path) {
			case 0:
				{
					const float* a = v + (tr.constant ? 0 : i0 * 3);
					const float* b = v + (tr.constant ? 0 : i1 * 3);
					a_out.t[tr.joint] = { a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w, a[2] + (b[2] - a[2]) * w };
					break;
				}
			case 1:
				{
					const float* a = v + (tr.constant ? 0 : i0 * 4);
					const float* b = v + (tr.constant ? 0 : i1 * 4);
					a_out.r[tr.joint] = Nlerp({ a[3], a[0], a[1], a[2] }, { b[3], b[0], b[1], b[2] }, w);
					break;
				}
			default:
				{
					const float a = v[tr.constant ? 0 : i0];
					const float b = v[tr.constant ? 0 : i1];
					a_out.s[tr.joint] = a + (b - a) * w;
					break;
				}
			}
		}
	}

	void Library::AddAdditive(const Clip& a_clip, float a_t, float a_weight, Pose& a_pose) const
	{
		if (a_weight <= 0.0f || a_pose.t.size() != _joints.size()) {
			return;
		}
		const float last = static_cast<float>(a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		const float f = std::clamp(a_t * a_clip.rate, 0.0f, last);
		const auto  i0 = static_cast<std::uint32_t>(f);
		const auto  i1 = std::min(i0 + 1, a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		const float w = f - static_cast<float>(i0);
		const RE::NiQuaternion identity{ 1.0f, 0.0f, 0.0f, 0.0f };
		for (const auto& tr : a_clip.tracks) {
			const float* v = tr.values.data();
			if (tr.path == 0) {
				const float* a = v + (tr.constant ? 0 : i0 * 3);
				const float* b = v + (tr.constant ? 0 : i1 * 3);
				const RE::NiPoint3 d{ a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w, a[2] + (b[2] - a[2]) * w };
				a_pose.t[tr.joint] = a_pose.t[tr.joint] + d * a_weight;
			} else if (tr.path == 1) {
				const float* a = v + (tr.constant ? 0 : i0 * 4);
				const float* b = v + (tr.constant ? 0 : i1 * 4);
				const RE::NiQuaternion d = Nlerp({ a[3], a[0], a[1], a[2] }, { b[3], b[0], b[1], b[2] }, w);
				// Bevy's additive rotation: slerp(identity, d, weight) * base.
				a_pose.r[tr.joint] = Multiply(Nlerp(identity, d, a_weight), a_pose.r[tr.joint]);
			}
			// Scale: none of his additive clips key it.
		}
	}

	void Blend(Pose& a_a, const Pose& a_b, float a_w)
	{
		if (a_w <= 0.0f || a_a.t.size() != a_b.t.size()) {
			return;
		}
		if (a_w >= 1.0f) {
			a_a = a_b;
			return;
		}
		for (std::size_t i = 0; i < a_a.t.size(); ++i) {
			a_a.t[i] = a_a.t[i] + (a_b.t[i] - a_a.t[i]) * a_w;
			a_a.r[i] = Nlerp(a_a.r[i], a_b.r[i], a_w);
			a_a.s[i] = a_a.s[i] + (a_b.s[i] - a_a.s[i]) * a_w;
		}
	}
}

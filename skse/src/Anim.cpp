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

		// glam's slerp (Bevy's interpolation of rotations): the shorter way, lerped when nearly
		// the same.
		RE::NiQuaternion Slerp(const RE::NiQuaternion& a_a, RE::NiQuaternion a_b, float a_t)
		{
			float dot = a_a.Dot(a_b);
			if (dot < 0.0f) {
				a_b.Neg();
				dot = -dot;
			}
			if (dot > 1.0f - 1.1920929e-7f) {
				return Normalized({ a_a.w + (a_b.w - a_a.w) * a_t, a_a.x + (a_b.x - a_a.x) * a_t, a_a.y + (a_b.y - a_a.y) * a_t,
					a_a.z + (a_b.z - a_a.z) * a_t });
			}
			const float theta = std::acos(std::clamp(dot, -1.0f, 1.0f));
			const float k1 = std::sin(theta * (1.0f - a_t)) / std::sin(theta);
			const float k2 = std::sin(theta * a_t) / std::sin(theta);
			return { a_a.w * k1 + a_b.w * k2, a_a.x * k1 + a_b.x * k2, a_a.y * k1 + a_b.y * k2, a_a.z * k1 + a_b.z * k2 };
		}

		RE::NiQuaternion Nlerp(const RE::NiQuaternion& a_a, RE::NiQuaternion a_b, float a_w)
		{
			if (a_a.Dot(a_b) < 0.0f) {
				a_b.Neg();
			}
			return Normalized({ a_a.w + (a_b.w - a_a.w) * a_w, a_a.x + (a_b.x - a_a.x) * a_w, a_a.y + (a_b.y - a_a.y) * a_w,
				a_a.z + (a_b.z - a_a.z) * a_w });
		}
	}

	RE::NiQuaternion Multiply(const RE::NiQuaternion& a_a, const RE::NiQuaternion& a_b)
	{
		return Normalized({ a_a.w * a_b.w - a_a.x * a_b.x - a_a.y * a_b.y - a_a.z * a_b.z,
			a_a.w * a_b.x + a_a.x * a_b.w + a_a.y * a_b.z - a_a.z * a_b.y,
			a_a.w * a_b.y - a_a.x * a_b.z + a_a.y * a_b.w + a_a.z * a_b.x,
			a_a.w * a_b.z + a_a.x * a_b.y - a_a.y * a_b.x + a_a.z * a_b.w });
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

	void Library::Overlay(const Clip& a_clip, float a_t, bool a_loop, float a_weight, Pose& a_pose) const
	{
		if (a_weight <= 0.0f || a_pose.t.size() != _joints.size()) {
			return;
		}
		const float last = static_cast<float>(a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		float       f = a_t * a_clip.rate;
		if (a_loop && last > 0.0f) {
			f = std::fmod(f, last);
		} else {
			f = std::clamp(f, 0.0f, last);
		}
		const auto  i0 = static_cast<std::uint32_t>(std::max(f, 0.0f));
		const auto  i1 = std::min(i0 + 1, a_clip.frames > 0 ? a_clip.frames - 1 : 0);
		const float w = f - static_cast<float>(i0);
		const float k = std::min(a_weight, 1.0f);
		for (const auto& tr : a_clip.tracks) {
			const float* v = tr.values.data();
			if (tr.path == 0) {
				const float*       a = v + (tr.constant ? 0 : i0 * 3);
				const float*       b = v + (tr.constant ? 0 : i1 * 3);
				const RE::NiPoint3 p{ a[0] + (b[0] - a[0]) * w, a[1] + (b[1] - a[1]) * w, a[2] + (b[2] - a[2]) * w };
				a_pose.t[tr.joint] = a_pose.t[tr.joint] + (p - a_pose.t[tr.joint]) * k;
			} else if (tr.path == 1) {
				const float* a = v + (tr.constant ? 0 : i0 * 4);
				const float* b = v + (tr.constant ? 0 : i1 * 4);
				a_pose.r[tr.joint] = Nlerp(a_pose.r[tr.joint], Nlerp({ a[3], a[0], a[1], a[2] }, { b[3], b[0], b[1], b[2] }, w), k);
			} else {
				const float a = v[tr.constant ? 0 : i0];
				const float b = v[tr.constant ? 0 : i1];
				a_pose.s[tr.joint] = a_pose.s[tr.joint] + (a + (b - a) * w - a_pose.s[tr.joint]) * k;
			}
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

	namespace
	{
		// A clip's two keys around a_t (clamped to the clip) and how far between them.
		struct Keys
		{
			std::uint32_t i0 = 0;
			std::uint32_t i1 = 0;
			float         w = 0.0f;
		};

		Keys KeysAt(const Clip& a_clip, float a_t)
		{
			const std::uint32_t last = a_clip.frames > 0 ? a_clip.frames - 1 : 0;
			const float         f = std::clamp(a_t * a_clip.rate, 0.0f, static_cast<float>(last));
			Keys                k;
			k.i0 = std::min(static_cast<std::uint32_t>(f), last);
			k.i1 = std::min(k.i0 + 1, last);
			k.w = f - static_cast<float>(k.i0);
			return k;
		}

		RE::NiPoint3 PointAt(const Track& a_track, const Keys& a_k)
		{
			const float* a = a_track.values.data() + (a_track.constant ? 0 : a_k.i0 * 3);
			const float* b = a_track.values.data() + (a_track.constant ? 0 : a_k.i1 * 3);
			return { a[0] + (b[0] - a[0]) * a_k.w, a[1] + (b[1] - a[1]) * a_k.w, a[2] + (b[2] - a[2]) * a_k.w };
		}

		RE::NiQuaternion TurnAt(const Track& a_track, const Keys& a_k)
		{
			const float* a = a_track.values.data() + (a_track.constant ? 0 : a_k.i0 * 4);
			const float* b = a_track.values.data() + (a_track.constant ? 0 : a_k.i1 * 4);
			return Slerp({ a[3], a[0], a[1], a[2] }, { b[3], b[0], b[1], b[2] }, a_k.w);
		}

		float ScaleAt(const Track& a_track, const Keys& a_k)
		{
			const float a = a_track.values[a_track.constant ? 0 : a_k.i0];
			const float b = a_track.values[a_track.constant ? 0 : a_k.i1];
			return a + (b - a) * a_k.w;
		}
	}

	void Library::SetUpperBody(std::string_view a_name)
	{
		_upper.assign(_joints.size(), false);
		// Parents come before their children in the file.
		for (std::size_t i = 0; i < _joints.size(); ++i) {
			const auto parent = _joints[i].parent;
			_upper[i] = _joints[i].name == a_name || (parent >= 0 && static_cast<std::size_t>(parent) < i && _upper[parent]);
		}
	}

	void Library::Mix(std::span<const Layer> a_layers, Pose& a_pose) const
	{
		const std::size_t n = _joints.size();
		if (a_pose.t.size() != n) {
			a_pose = Rest();
		}
		// The weight blended into each channel so far (joint × 3).
		thread_local std::vector<float> sums;
		sums.assign(n * 3, 0.0f);
		for (const Layer& layer : a_layers) {
			if (!layer.clip || layer.weight <= 0.0f || layer.kind == Layer::Kind::kAdditive) {
				continue;
			}
			const bool upperOnly = layer.kind == Layer::Kind::kBaseUpperBody && _upper.size() == n;
			const Keys k = KeysAt(*layer.clip, layer.time);
			for (const Track& tr : layer.clip->tracks) {
				if (upperOnly && !_upper[tr.joint]) {
					continue;
				}
				float& sum = sums[tr.joint * 3 + tr.path];
				sum += layer.weight;
				const float w = layer.weight / sum;  // 1 for the first: it sets the channel
				switch (tr.path) {
				case 0:
					a_pose.t[tr.joint] = a_pose.t[tr.joint] + (PointAt(tr, k) - a_pose.t[tr.joint]) * w;
					break;
				case 1:
					a_pose.r[tr.joint] = w >= 1.0f ? TurnAt(tr, k) : Slerp(a_pose.r[tr.joint], TurnAt(tr, k), w);
					break;
				default:
					a_pose.s[tr.joint] = a_pose.s[tr.joint] + (ScaleAt(tr, k) - a_pose.s[tr.joint]) * w;
					break;
				}
			}
		}
		const RE::NiQuaternion identity{ 1.0f, 0.0f, 0.0f, 0.0f };
		for (const Layer& layer : a_layers) {
			if (!layer.clip || layer.weight <= 0.0f || layer.kind != Layer::Kind::kAdditive) {
				continue;
			}
			const Keys k = KeysAt(*layer.clip, layer.time);
			for (const Track& tr : layer.clip->tracks) {
				switch (tr.path) {
				case 0:
					a_pose.t[tr.joint] = a_pose.t[tr.joint] + PointAt(tr, k) * layer.weight;
					break;
				case 1:
					a_pose.r[tr.joint] = Multiply(Slerp(identity, TurnAt(tr, k), layer.weight), a_pose.r[tr.joint]);
					break;
				default:
					a_pose.s[tr.joint] = a_pose.s[tr.joint] + ScaleAt(tr, k) * layer.weight;
					break;
				}
			}
		}
	}

	void Player::Play(const Clip* a_clip, float a_fade, bool a_loop)
	{
		if (!a_clip) {
			return;
		}
		if (_main.clip) {
			// It fades out from the weight it has now.
			Playing old = _main;
			old.decline = a_fade > 0.0f ? 1.0f / a_fade : std::numeric_limits<float>::infinity();
			_fading.push_back(old);
		}
		std::erase_if(_fading, [&](const Playing& a_p) { return a_p.clip == a_clip; });
		_main = { a_clip, 0.0f, a_loop, 1.0f, 0.0f };
	}

	void Player::Advance(float a_dt)
	{
		// The weights (Bevy's advance_transitions), the newest fading clip first: each keeps its
		// own weight × what the newer ones left, and the clip playing gets the rest. The faded
		// ones go (its expire_completed_transitions).
		float remaining = 1.0f;
		for (auto it = _fading.rbegin(); it != _fading.rend(); ++it) {
			it->weight = std::max(0.0f, it->weight - it->decline * a_dt);
			const float share = it->weight * remaining;
			remaining -= share;
			it->share = share;
		}
		_main.weight = remaining;
		_main.share = remaining;
		std::erase_if(_fading, [](const Playing& a_p) { return a_p.weight <= 0.0f; });
		// The clocks (its advance_animations): a looping clip wraps, a one-shot holds at its end.
		const auto tick = [a_dt](Playing& a_p) {
			if (!a_p.clip || a_p.clip->seconds <= 0.0f || (!a_p.loop && a_p.time >= a_p.clip->seconds)) {
				return;
			}
			a_p.time += a_dt;
			if (a_p.loop && a_p.time >= a_p.clip->seconds) {
				a_p.time = std::fmod(a_p.time, a_p.clip->seconds);
			}
		};
		tick(_main);
		for (auto& p : _fading) {
			tick(p);
		}
	}

	void Player::Layers(std::vector<Layer>& a_out) const
	{
		for (const auto& p : _fading) {
			a_out.push_back({ p.clip, p.time, p.share, Layer::Kind::kBase });
		}
		if (_main.clip) {
			a_out.push_back({ _main.clip, _main.time, _main.share, Layer::Kind::kBase });
		}
	}
}

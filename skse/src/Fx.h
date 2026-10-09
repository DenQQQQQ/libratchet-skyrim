// SPDX-License-Identifier: GPL-3.0-or-later
#pragma once

// Rift Apart's weapon effects as the sandbox draws them (apps/sandbox/src/vfx.rs): the game's own
// effect art (skse/tools/ratchet_vfx_to_skyrim.py makes Skyrim files of it) with the sandbox's
// timing, sizes, motion and colours. Places are Skyrim's (game units), directions Skyrim's axes;
// sizes are the sandbox's metres at his size (× INI fScale).
namespace Fx
{
	// Whether the converted art is installed (looked for once).
	bool Available();
	// Once a frame: the pieces move, grow, fade and turn to the camera; spent ones go.
	void Update(float a_dt);
	// All of them gone (a load, a new world).
	void Clear();
	// For the tests: the pieces and bolts alive, and whether they hang under the player's cell's 3D.
	std::size_t Live();
	bool        UnderPlayersCell();

	// The sandbox's crosshair, a "+" drawn over everything: a_at is its quad's world transform
	// (in front of the camera, facing it, its size in its scale); nullopt hides it.
	void Crosshair(const std::optional<RE::NiTransform>& a_at);

	// The Burst Pistol's flash at its muzzle, shooting along a_dir (unit).
	void MuzzleFlash(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir);
	// A bolt's look leaving the muzzle at a_at along a_dir: its head (the game's tracer and glow
	// shape) and the streak growing out behind it. MoveBolt moves it on; EndBolt ends it where it
	// is (its streak catches up and goes).
	std::uint32_t Bolt(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir);
	void          MoveBolt(std::uint32_t a_bolt, const RE::NiPoint3& a_at);
	void          EndBolt(std::uint32_t a_bolt);
	// The bolt's smoke trail over one step of its flight.
	void BoltTrail(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to);
	// Where a bolt hits (a_normal out of what it hit, unit): the flare, a glow, sparks bouncing
	// off, a puff of smoke, and on level geometry (a_mark) the bullet mark glowing green a moment.
	void BoltImpact(const RE::NiPoint3& a_at, const RE::NiPoint3& a_normal, bool a_mark);
	// A spent shell out of the gun's ejector (a_turn: the ejector's; its z points out of the gun),
	// bouncing on the ground at the height a_floor if there's one.
	void Shell(const RE::NiPoint3& a_at, const RE::NiMatrix3& a_turn, std::optional<float> a_floor);

	// While a Shatterbomb throw winds up, where it would fly (the sandbox's throw_arc): its
	// flight's points (game units, 0.03 s apart) dashed, and a ring a_radius (game units) round
	// where it lands, if it does. Drawn for this frame only.
	void ThrowArc(std::span<const RE::NiPoint3> a_points, std::optional<RE::NiPoint3> a_landing, float a_radius);

	// A Shatterbomb in flight, a_id its throw: its violet glow lights what's near (the sandbox's
	// grenade_light). Placed each frame it flies; it goes out when it isn't.
	void BombGlow(std::uint32_t a_id, const RE::NiPoint3& a_at);

	// The Shatterbomb going off (a_radius: the blast's, game units), a_ground the ground's height
	// under it if it's near.
	void Explosion(const RE::NiPoint3& a_at, float a_radius, std::optional<float> a_ground);

	// The wrench's swing streak (the sandbox's SwipeStreak): each frame he swings, where the wrench
	// in his hand is (its world transform: its handle along its y); a ribbon through where its head
	// has been over the last 0.12 s, fading with age. nullopt when he isn't swinging.
	void Swipe(std::optional<RE::NiTransform> a_wrench);

	// A wrench hit (a_dir from him to the target, unit), its sparks bouncing at a_floor.
	void WrenchHit(const RE::NiPoint3& a_at, const RE::NiPoint3& a_dir, std::optional<float> a_floor);
	// The Hyper-Strike landing at a_at on the ground (a_radius: its reach, game units).
	void WrenchSlam(const RE::NiPoint3& a_at, float a_radius);
	// The thrown wrench's trail over one step of its flight.
	void WrenchTrail(const RE::NiPoint3& a_from, const RE::NiPoint3& a_to);
}

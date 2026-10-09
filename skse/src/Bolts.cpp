// SPDX-License-Identifier: GPL-3.0-or-later
#include "Bolts.h"

#include <unordered_set>

#include "AutoTest.h"
#include "Combat.h"
#include "Fx.h"
#include "Hud.h"
#include "Physics.h"
#include "Settings.h"
#include "Sounds.h"

namespace Bolts
{
	namespace
	{
		// The sandbox's (bolts.rs): the bolts' randomness starts here, so runs repeat.
		constexpr std::uint32_t kSeed = 0xB017;
		// They burst out this far above a kill's feet (the game's, for an enemy with no spawn
		// locator), × his size.
		constexpr float kDropHeight = 1.0f;
		// Where they fly to: the middle of his body, half the sandbox's body height (1.2 m), × his size.
		constexpr float kBodyMiddle = 0.6f;
		// They step at most this long at a time (the sandbox's run at 120 Hz; a flying bolt's ray is
		// one step's move).
		constexpr float kMaxStep = 1.0f / 60.0f;
		// Rift Apart's relative values run from 1 (small enemies) to 30 (bosses).
		constexpr float kMinValue = 1.0f;
		constexpr float kMaxValue = 30.0f;
		// The glint (bolts.rs, all the sandbox's own): a twinkle this long and this wide (metres at
		// his size), every so often, its size over its life by the game's curve as read.
		constexpr float                   kGlintLife = 0.35f;
		constexpr float                   kGlintSize = 0.3f;
		constexpr float                   kGlintEvery[2] = { 0.8f, 2.5f };
		constexpr std::pair<float, float> kGlintCurve[] = { { 0.0f, 0.5f }, { 0.186f, 1.0f }, { 0.399f, 1.0f }, { 0.625f, 0.467f },
			{ 0.796f, 0.172f }, { 1.0f, 0.0f } };

		// A kill due to drop its bolts (the death event can come off the main thread).
		struct Death
		{
			RE::ActorHandle actor;
			RE::NiPoint3    at;
			std::string     name;
		};

		struct State
		{
			RatchetBolts* world = nullptr;
			RatchetTally* tally = nullptr;
			std::uint32_t total = 0;  // without a tally (ratchet.dll not loaded)
			std::mutex                     lock;
			std::vector<Death>             deaths;
			std::unordered_set<RE::FormID> dying;  // his kills seen dying: their "dead" event drops nothing
			std::uint32_t                  dropped = 0;
			std::uint32_t                  collected = 0;
			std::uint32_t                  kills = 0;
			std::unordered_map<std::uint32_t, float> twinkle;  // a pickup's seconds until its twinkle (below 0: in it)
			std::uint32_t                  random = 0x2545F491u;
			std::optional<RE::NiPoint3>    lastAt;
			const void*                    lastWorld = nullptr;
			bool                           flying = false;
			float                          sinceDrop = 100.0f;  // seconds since the last kill's drop
			std::optional<RatchetTallyView> onHud;  // the counter as last put on the HUD
			std::vector<RatchetBoltPickup>  pickups;
			std::vector<RatchetBoltCollected> got;
		};
		State g;

		float Units() { return RE::bhkWorld::GetWorldScaleInverse(); }

		RatchetVec3 ToSim(const RE::NiPoint3& a_units)
		{
			const float k = 1.0f / Units();
			return { a_units.x * k, a_units.z * k, -a_units.y * k };
		}

		RE::NiPoint3 FromSim(const RatchetVec3& a_metres)
		{
			const float u = Units();
			return { a_metres.x * u, -a_metres.z * u, a_metres.y * u };
		}

		float Random(float a_lo, float a_hi)
		{
			g.random ^= g.random << 13;
			g.random ^= g.random >> 17;
			g.random ^= g.random << 5;
			return a_lo + (a_hi - a_lo) * (static_cast<float>(g.random >> 8) / 16777216.0f);
		}

		// The glint's size a_t (0..1) into its twinkle.
		float GlintSize(float a_t)
		{
			for (std::size_t i = 1; i < std::size(kGlintCurve); ++i) {
				const auto [t1, v1] = kGlintCurve[i];
				if (a_t < t1 || i + 1 == std::size(kGlintCurve)) {
					const auto [t0, v0] = kGlintCurve[i - 1];
					return v0 + (v1 - v0) * std::clamp((a_t - t0) / (t1 - t0), 0.0f, 1.0f);
				}
			}
			return 0.0f;
		}

		// Flying bolts against Skyrim's world: what he collides with, through bodies (the sandbox's
		// bolts meet the level only; a ray starting inside a body doesn't see it).
		std::uint8_t Raycast(void*, RatchetVec3 a_from, RatchetVec3 a_to, RatchetVec3* a_point, RatchetVec3* a_normal)
		{
			RE::NiPoint3       from = FromSim(a_from);
			const RE::NiPoint3 to = FromSim(a_to);
			for (int i = 0; i < 4; ++i) {
				const auto hit = Physics::Ray(from, to);
				if (!hit) {
					return 0;
				}
				if (hit->ref && hit->ref->As<RE::Actor>()) {
					RE::NiPoint3 way = to - from;
					const float  length = way.Length();
					if (length < 1e-3f) {
						return 0;
					}
					from = hit->point + way / length;  // a game unit into it, and on
					continue;
				}
				*a_point = ToSim(hit->point);
				*a_normal = { hit->normal.x, hit->normal.z, -hit->normal.y };
				return 1;
			}
			return 0;
		}

		struct DeathSink final : RE::BSTEventSink<RE::TESDeathEvent>
		{
			RE::BSEventNotifyControl ProcessEvent(const RE::TESDeathEvent* a_event, RE::BSTEventSource<RE::TESDeathEvent>*) override
			{
				auto* player = RE::PlayerCharacter::GetSingleton();
				auto* dying = a_event && a_event->actorDying ? a_event->actorDying->As<RE::Actor>() : nullptr;
				if (!dying || !player || dying == player || a_event->actorKiller.get() != player) {
					return RE::BSEventNotifyControl::kContinue;
				}
				std::scoped_lock lock(g.lock);
				// A death comes as "dying", then "dead": its bolts burst at the first (one with
				// only the second drops them then).
				const RE::FormID id = dying->GetFormID();
				if (!a_event->dead) {
					g.dying.insert(id);
				} else if (g.dying.erase(id) > 0) {
					return RE::BSEventNotifyControl::kContinue;
				}
				g.deaths.push_back({ dying->GetHandle(), dying->GetPosition(), dying->GetName() });
				return RE::BSEventNotifyControl::kContinue;
			}
		};

		// His kills since the last frame: each one's reward bursts out of it.
		void Drop()
		{
			std::vector<Death> deaths;
			{
				std::scoped_lock lock(g.lock);
				deaths.swap(g.deaths);
			}
			const auto& s = Settings::Get();
			for (const auto& d : deaths) {
				const auto   actor = d.actor.get();
				RE::NiPoint3 at = actor ? actor->GetPosition() : d.at;
				const float  health = actor ? actor->AsActorValueOwner()->GetPermanentActorValue(RE::ActorValue::kHealth) : 0.0f;
				// Its worth: Rift Apart's relative value from how tough it was (ours).
				const float value = std::clamp(health / s.boltHealthPerValue, kMinValue, kMaxValue);
				at.z += kDropHeight * s.scale * Units();
				const std::uint32_t n = ratchet_bolts_drop_reward(g.world, ToSim(at), value, s.boltLevelMultiplier);
				g.dropped += n;
				++g.kills;
				g.sinceDrop = 0.0f;
				SKSE::log::info("bolts: {} went down ({:.0f} health, worth {:.1f}): {} bolts burst out", d.name, health, value, n);
			}
		}

		// Carried off (a load door, fast travel, a load): the bolts lying in the old place go.
		void ClearIfCarriedOff(RE::PlayerCharacter* a_player)
		{
			const RE::NiPoint3 at = a_player->GetPosition();
			const void*        world = a_player->GetWorldspace() ? static_cast<const void*>(a_player->GetWorldspace()) :
			                                                       static_cast<const void*>(a_player->GetParentCell());
			if ((g.lastAt && g.lastAt->GetDistance(at) > 30.0f * Units()) || (g.lastWorld && world != g.lastWorld)) {
				if (const auto left = ratchet_bolts_pickups(g.world, nullptr, 0); left > 0) {
					SKSE::log::info("bolts: he was carried off: {} pickups left behind", left);
				}
				ratchet_bolts_clear(g.world);
				g.twinkle.clear();
			}
			g.lastAt = at;
			g.lastWorld = world;
		}

		// What the last step collected: his, with the game's collect sound for each.
		void Collect(const RE::NiPoint3& a_feet)
		{
			const std::uint32_t n = ratchet_bolts_collected(g.world, nullptr, 0);
			if (n == 0) {
				return;
			}
			g.got.resize(n);
			ratchet_bolts_collected(g.world, g.got.data(), n);
			for (const auto& c : g.got) {
				ratchet_tally_add(g.tally, c.bolts);
				g.collected += c.bolts;
				Sounds::Play("bolt_collect", a_feet);
			}
		}

		// Each pickup where it is, turned and bobbing, with its twinkle.
		void Draw(float a_dt)
		{
			const std::uint32_t n = ratchet_bolts_pickups(g.world, nullptr, 0);
			g.pickups.resize(n);
			if (n > 0) {
				ratchet_bolts_pickups(g.world, g.pickups.data(), n);
			}
			for (const auto& p : g.pickups) {
				auto [it, fresh] = g.twinkle.try_emplace(p.id, 0.0f);
				float& twinkle = it->second;
				if (fresh) {
					twinkle = Random(0.0f, kGlintEvery[1]);
				}
				twinkle -= a_dt;
				if (twinkle < -kGlintLife) {
					twinkle = Random(kGlintEvery[0], kGlintEvery[1]);
				}
				const float glint = twinkle < 0.0f ? GlintSize(-twinkle / kGlintLife) * kGlintSize : 0.0f;
				Fx::Pickup(p.id, p.tier, p.look, FromSim(p.position), p.rotation, glint);
			}
			if (g.twinkle.size() > g.pickups.size()) {
				std::erase_if(g.twinkle, [](const auto& a_entry) {
					return std::ranges::none_of(g.pickups, [&](const RatchetBoltPickup& a_p) { return a_p.id == a_entry.first; });
				});
			}
			Fx::EndPickups();
			const bool flying = std::ranges::any_of(g.pickups, [](const RatchetBoltPickup& a_p) { return a_p.state == RATCHET_BOLT_FLYING; });
			if (g.flying && !flying) {
				SKSE::log::info("bolts: all landed {:.2f} s after the last drop ({} lying)", g.sinceDrop,
					std::ranges::count_if(g.pickups, [](const RatchetBoltPickup& a_p) { return a_p.state == RATCHET_BOLT_RESTING; }));
				AutoTest::OnMove("bolts_landed");
			}
			g.flying = flying;
			// (A moment into a burst, for the tests' pictures.)
			if (g.sinceDrop < 0.3f && g.sinceDrop + a_dt >= 0.3f) {
				AutoTest::OnMove("bolts_flying");
			}
			g.sinceDrop += a_dt;
		}

		// Rift Apart's counter, put on the HUD when what it shows changes.
		void ShowCounter()
		{
			if (!Settings::Get().boltCounter || !g.tally) {
				return;
			}
			const RatchetTallyView view = ratchet_tally_view(g.tally);
			if (g.onHud && g.onHud->shown == view.shown && g.onHud->gain == view.gain && g.onHud->visible == view.visible) {
				return;
			}
			g.onHud = view;
			Hud::BoltCounter(view.visible != 0, view.shown, view.gain);
		}
	}

	void Install()
	{
		const auto& s = Settings::Get();
		if (!s.bolts || !Ratchet::Loaded()) {
			SKSE::log::info("bolts: off");
			return;
		}
		g.world = ratchet_bolts_new(s.scale, kSeed);
		if (!g.tally) {
			g.tally = ratchet_tally_new(g.total);
		}
		static DeathSink sink;
		RE::ScriptEventSourceHolder::GetSingleton()->AddEventSink<RE::TESDeathEvent>(&sink);
		SKSE::log::info("bolts: on: what he kills drops them (a kill's max health / {:.0f} = its worth, 1 to 30, x {:.0f} bolts); the counter {}",
			s.boltHealthPerValue, s.boltLevelMultiplier, s.boltCounter ? "on the HUD" : "off");
	}

	void Reset()
	{
		if (g.world) {
			ratchet_bolts_clear(g.world);
		}
		{
			std::scoped_lock lock(g.lock);
			g.deaths.clear();
			g.dying.clear();
		}
		g.twinkle.clear();
		g.lastAt.reset();
		g.lastWorld = nullptr;
		g.flying = false;
		g.onHud.reset();
	}

	void Update(RE::PlayerCharacter* a_player, float a_dt)
	{
		if (!g.world || !g.tally || !a_player || a_dt <= 0.0f) {
			return;
		}
		ClearIfCarriedOff(a_player);
		Drop();
		const float        s = Settings::Get().scale;
		const RE::NiPoint3 feet = a_player->GetPosition();
		RatchetBoltHero    hero{};
		hero.position = ToSim(feet);
		hero.center = ToSim(feet + RE::NiPoint3{ 0.0f, 0.0f, kBodyMiddle * s * Units() });
		if (const auto wrench = Combat::ThrownWrench()) {
			hero.has_wrench = 1;
			hero.wrench = ToSim(wrench->first);
		}
		const int   steps = std::max(1, static_cast<int>(std::ceil(a_dt / kMaxStep)));
		const float step = a_dt / static_cast<float>(steps);
		for (int i = 0; i < steps; ++i) {
			ratchet_bolts_step(g.world, step, hero, Raycast, nullptr);
			Collect(feet);
		}
		ratchet_tally_step(g.tally, a_dt);
		Draw(a_dt);
		ShowCounter();
	}

	std::uint32_t Total() { return g.tally ? ratchet_tally_view(g.tally).total : g.total; }

	void SetTotal(std::uint32_t a_total)
	{
		g.total = std::min<std::uint32_t>(a_total, RATCHET_BOLTS_MAX);
		if (!Ratchet::Loaded()) {
			return;
		}
		// The counter starts over, showing the save's total (hidden till it changes).
		if (g.tally) {
			ratchet_tally_free(g.tally);
		}
		g.tally = ratchet_tally_new(g.total);
		g.onHud.reset();
	}

	Stats Count()
	{
		Stats out{ g.dropped, g.collected, g.kills };
		if (!g.world) {
			return out;
		}
		const std::uint32_t n = ratchet_bolts_pickups(g.world, nullptr, 0);
		std::vector<RatchetBoltPickup> pickups(n);
		if (n > 0) {
			ratchet_bolts_pickups(g.world, pickups.data(), n);
		}
		out.lowestAboveGround = 0.0f;
		bool any = false;
		for (const auto& p : pickups) {
			switch (p.state) {
			case RATCHET_BOLT_QUEUED:
				++out.queued;
				break;
			case RATCHET_BOLT_FLYING:
				++out.flying;
				break;
			case RATCHET_BOLT_RESTING:
				{
					++out.resting;
					if (const auto ground = Physics::GroundDistance(FromSim(p.position), 3.0f)) {
						out.lowestAboveGround = any ? std::min(out.lowestAboveGround, *ground) : *ground;
						any = true;
					}
					break;
				}
			default:
				++out.pulled;
				break;
			}
		}
		return out;
	}

	RatchetTallyView Counter() { return g.tally ? ratchet_tally_view(g.tally) : RatchetTallyView{}; }
}

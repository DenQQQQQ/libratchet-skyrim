// Checks that the C header matches the Rust library: jump on a flat floor and print the apex,
// then swing the wrench and throw it, then animate a walk into a run, then drop a dummy's bolts
// and collect them.
#include "ratchet.h"
#include <algorithm>
#include <cstddef>
#include <cstdio>
#include <string>
#include <vector>

// The animation's structs as the Rust side lays them out (crates/libratchet-ffi/src/anim.rs).
static_assert(sizeof(RatchetAnimLayer) == 12 && offsetof(RatchetAnimLayer, time) == 4, "RatchetAnimLayer");
static_assert(sizeof(RatchetAnimClock) == 12 && offsetof(RatchetAnimClock, previous) == 4, "RatchetAnimClock");
static_assert(sizeof(RatchetAnimInput) == 92 && offsetof(RatchetAnimInput, aim_target) == 68, "RatchetAnimInput");
static_assert(offsetof(RatchetClipInfo, seconds) == sizeof(const char*), "RatchetClipInfo");
// The bolts' (crates/libratchet-ffi/src/bolts.rs).
static_assert(sizeof(RatchetBoltPickup) == 36 && offsetof(RatchetBoltPickup, position) == 8 && offsetof(RatchetBoltPickup, rotation) == 20, "RatchetBoltPickup");
static_assert(sizeof(RatchetBoltCollected) == 24 && offsetof(RatchetBoltCollected, bolts) == 8 && offsetof(RatchetBoltCollected, at) == 12, "RatchetBoltCollected");
static_assert(sizeof(RatchetBoltHero) == 40 && offsetof(RatchetBoltHero, has_wrench) == 24 && offsetof(RatchetBoltHero, wrench) == 28, "RatchetBoltHero");
static_assert(sizeof(RatchetTallyView) == 16 && offsetof(RatchetTallyView, visible) == 12, "RatchetTallyView");

// A floor at y = 0, for the bolts.
static uint8_t floor_ray(void*, RatchetVec3 from, RatchetVec3 to, RatchetVec3* point, RatchetVec3* normal) {
    if (from.y < 0.0f || to.y >= 0.0f) return 0;
    const float t = from.y / (from.y - to.y);
    *point = RatchetVec3{from.x + (to.x - from.x) * t, 0.0f, from.z + (to.z - from.z) * t};
    *normal = RatchetVec3{0.0f, 1.0f, 0.0f};
    return 1;
}

int main() {
    if (ratchet_api_version() != RATCHET_API_VERSION) {
        std::printf("library API %u, header %u\n", ratchet_api_version(), RATCHET_API_VERSION);
        return 1;
    }
    RatchetSim* sim = ratchet_new(1.0f);
    RatchetVec3 feet{0, 0, 0};
    RatchetContact contact{1, 0, 1, 0.0f};
    float apex = 0;
    uint32_t seen = 0;
    for (int i = 0; i < 240; ++i) {
        RatchetInput in{0.0f, 1.0f, static_cast<uint8_t>(i < 60), 0, 0};
        RatchetStepResult r = ratchet_step(sim, 1.0f / 120.0f, in, contact, feet);
        seen |= r.events;
        feet.x += r.velocity.x / 120.0f;
        feet.y = std::max(0.0f, feet.y + r.velocity.y / 120.0f);
        feet.z += r.velocity.z / 120.0f;
        apex = std::max(apex, feet.y);
        contact = RatchetContact{static_cast<uint8_t>(feet.y <= 0.0f), 0, 1, feet.y};
    }

    // Melee on the ground: one swing, with its hit.
    uint32_t melee_seen = 0;
    for (int i = 0; i < 120; ++i) {
        RatchetInput in{0.0f, 0.0f, 0, 0, static_cast<uint8_t>(i == 0)};
        RatchetStepResult r = ratchet_step(sim, 1.0f / 120.0f, in, contact, feet);
        melee_seen |= r.events;
    }
    RatchetSwingInfo info{};
    uint32_t swings = ratchet_swing_info(sim, 0, &info);

    // Throw the wrench and wait for it to come back.
    RatchetWrench* wrench = ratchet_wrench_throw(sim, RatchetVec3{0, 1, 0}, RatchetVec3{0, 0, 1});
    float far = 0;
    int ticks = 0;
    for (; ticks < 1200; ++ticks) {
        RatchetVec3 at{};
        float spin = 0;
        if (ratchet_wrench_step(wrench, 1.0f / 120.0f, RatchetVec3{0, 1, 0}, 0, &at, &spin)) break;
        far = std::max(far, at.z);
    }
    ratchet_wrench_free(wrench);
    ratchet_free(sim);

    // The animation, with a few clips a second long (made up): standing, then the stick ahead at
    // 60 frames a second, two sim ticks each: the idle, the walk, the run.
    const char* names[] = {"idle", "walk", "run", "jump", "fall", "land"};
    std::vector<RatchetClipInfo> clips;
    for (const char* name : names) clips.push_back(RatchetClipInfo{name, 1.0f, 0.0f, -1.0f});
    RatchetAnim* anim = ratchet_anim_new(clips.data(), static_cast<uint32_t>(clips.size()));
    RatchetSim* runner = ratchet_new(1.0f);
    RatchetVec3 at{0, 0, 0};
    RatchetContact floor{1, 0, 1, 0.0f};
    std::vector<std::string> picks;
    RatchetAnimLayer layers[6];
    float total = 0;
    for (int frame = 0; frame < 60; ++frame) {
        RatchetAnimInput step{};
        step.dt = 1.0f / 60.0f;
        for (int tick = 0; tick < 2; ++tick) {
            RatchetInput in{0.0f, frame >= 10 ? 1.0f : 0.0f};
            RatchetStepResult r = ratchet_step(runner, 1.0f / 120.0f, in, floor, at);
            at.z += r.velocity.z / 120.0f;
            step.velocity = r.velocity;
            step.events |= r.events;
        }
        step.feet = at;
        step.has_ground_distance = 1;
        uint32_t n = ratchet_anim_update(anim, runner, &step, layers, 6);
        total = 0;
        for (uint32_t i = 0; i < n; ++i) total += layers[i].weight;
        if (picks.empty() || picks.back() != ratchet_anim_clip_name(anim)) picks.push_back(ratchet_anim_clip_name(anim));
    }
    RatchetAnimClock clock{};
    bool clocked = ratchet_anim_clock(anim, &clock) == 1;
    ratchet_anim_free(anim);
    ratchet_free(runner);

    // The sandbox's dummy (worth 2, the default economy): 20 bolts burst out 1 m up, land, and
    // swarm into a hero 4 m off; each is counted.
    RatchetBolts* bolts = ratchet_bolts_new(1.0f, 0xB017u);
    RatchetTally* tally = ratchet_tally_new(0);
    const uint32_t dropped = ratchet_bolts_drop_reward(bolts, RatchetVec3{0, 1, 0}, 2.0f, RATCHET_BOLTS_LEVEL_MULTIPLIER);
    const uint32_t pickups = ratchet_bolts_pickups(bolts, nullptr, 0);
    RatchetBoltHero hero{RatchetVec3{4, 0, 0}, RatchetVec3{4, 0.6f, 0}, 0, RatchetVec3{}};
    uint32_t got = 0;
    for (int i = 0; i < 1200; ++i) {
        ratchet_bolts_step(bolts, 1.0f / 120.0f, hero, floor_ray, nullptr);
        RatchetBoltCollected c[64];
        const uint32_t n = std::min(ratchet_bolts_collected(bolts, c, 64), 64u);
        for (uint32_t k = 0; k < n; ++k) {
            ratchet_tally_add(tally, c[k].bolts);
            got += c[k].bolts;
        }
        ratchet_tally_step(tally, 1.0f / 120.0f);
    }
    const RatchetTallyView counter = ratchet_tally_view(tally);
    const uint32_t left = ratchet_bolts_pickups(bolts, nullptr, 0);
    ratchet_tally_free(tally);
    ratchet_bolts_free(bolts);

    std::printf("apex %.3f m, moved %.2f m forward, events seen 0x%x\n", apex, feet.z, seen);
    std::printf("melee events 0x%x, %u swings (first hits at %.2f s), wrench went %.1f m and was caught after %.2f s\n",
                melee_seen, swings, info.hit_time, far, ticks / 120.0f);
    std::string order;
    for (const std::string& p : picks) order += (order.empty() ? "" : " > ") + p;
    std::printf("animation: %s; the run's clock at %.3f s\n", order.c_str(), clock.time);
    std::printf("bolts: %u dropped in %u pickups, %u collected, %u left; the counter shows %u\n", dropped, pickups, got, left, counter.shown);
    bool jump_ok = apex > 1.5f && apex < 1.7f && (seen & RATCHET_EVENT_JUMPED) && (seen & RATCHET_EVENT_LANDED);
    bool melee_ok = (melee_seen & RATCHET_EVENT_MELEE_SWING) && (melee_seen & RATCHET_EVENT_MELEE_HIT) && swings == 3;
    bool throw_ok = far > 5.0f && ticks < 1200;
    bool anim_ok = picks == std::vector<std::string>{"idle", "walk", "run"} && clocked && clock.clip == 2 && !clock.restarted &&
                   total > 0.999f && total < 1.001f;
    bool bolts_ok = dropped == 20 && pickups == 20 && got == 20 && left == 0 && counter.total == 20 && counter.shown == 20;
    return (jump_ok && melee_ok && throw_ok && anim_ok && bolts_ok) ? 0 : 1;
}

// Checks that the C header matches the Rust library: jump on a flat floor and print the apex,
// then swing the wrench and throw it.
#include "ratchet.h"
#include <algorithm>
#include <cstdio>

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

    std::printf("apex %.3f m, moved %.2f m forward, events seen 0x%x\n", apex, feet.z, seen);
    std::printf("melee events 0x%x, %u swings (first hits at %.2f s), wrench went %.1f m and was caught after %.2f s\n",
                melee_seen, swings, info.hit_time, far, ticks / 120.0f);
    bool jump_ok = apex > 1.5f && apex < 1.7f && (seen & RATCHET_EVENT_JUMPED) && (seen & RATCHET_EVENT_LANDED);
    bool melee_ok = (melee_seen & RATCHET_EVENT_MELEE_SWING) && (melee_seen & RATCHET_EVENT_MELEE_HIT) && swings == 3;
    bool throw_ok = far > 5.0f && ticks < 1200;
    return (jump_ok && melee_ok && throw_ok) ? 0 : 1;
}

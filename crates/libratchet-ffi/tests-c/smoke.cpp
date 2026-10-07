// Checks that the C header matches the Rust library: jump on a flat floor and print the apex.
#include "ratchet.h"
#include <algorithm>
#include <cstdio>

int main() {
    RatchetSim* sim = ratchet_new(1.0f);
    RatchetVec3 feet{0, 0, 0};
    RatchetContact contact{1, 0, 1, 0.0f};
    float apex = 0;
    uint32_t seen = 0;
    for (int i = 0; i < 240; ++i) {
        RatchetInput in{0.0f, 1.0f, static_cast<uint8_t>(i < 60), 0};
        RatchetStepResult r = ratchet_step(sim, 1.0f / 120.0f, in, contact, feet);
        seen |= r.events;
        feet.x += r.velocity.x / 120.0f;
        feet.y = std::max(0.0f, feet.y + r.velocity.y / 120.0f);
        feet.z += r.velocity.z / 120.0f;
        apex = std::max(apex, feet.y);
        contact = RatchetContact{static_cast<uint8_t>(feet.y <= 0.0f), 0, 1, feet.y};
    }
    ratchet_free(sim);
    std::printf("apex %.3f m, moved %.2f m forward, events seen 0x%x\n", apex, feet.z, seen);
    return (apex > 1.5f && apex < 1.7f && (seen & RATCHET_EVENT_JUMPED) && (seen & RATCHET_EVENT_LANDED)) ? 0 : 1;
}

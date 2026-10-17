#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <initializer_list>
#include <vector>

namespace ra3::ai {

constexpr uint32_t kMagic = 0x49413352; // bytes: R3AI
constexpr uint16_t kVersion = 2;
constexpr size_t kRequestSize = 16;
constexpr size_t kResponseHeaderSize = 48;
constexpr size_t kAgentPrefixSize = 212;
constexpr size_t kMaxAgents = 20;
constexpr size_t kMaxStrategicStates = 64;
constexpr uint32_t kInvalidPlayerSlot = 0xffffffffu;
constexpr uint32_t kAllAgentSlotsMask = (1u << kMaxAgents) - 1u;

using RequestPacket = std::array<uint8_t, kRequestSize>;

enum class Op : uint16_t { Snapshot = 0 };
enum class Status : uint16_t { Ok = 0, Invalid = 1, NotReady = 2 };

struct Request {
    Op op = Op::Snapshot;
    uint32_t sequence = 0;
};

struct StrategicState {
    uint32_t definition = 0;
    uint32_t scoreBits = 0;
    uint32_t flags = 0;
};

struct AgentSnapshot {
    uint32_t flags = 0;
    uint32_t captureSequence = 0;
    uint32_t gameFrame = 0;
    uint64_t capturedUnixMs = 0;

    uint32_t skirmishAi = 0;
    uint32_t bank = 0;
    uint32_t buildManager = 0;
    uint32_t buildStateManager = 0;
    uint32_t strategicStateManager = 0;
    uint32_t strategyManager = 0;
    uint32_t economyManager = 0;
    uint32_t player = 0;
    uint32_t armyDefinition = 0;
    uint32_t personalityRef = 0;
    uint32_t personalityDefinition = 0;

    uint32_t playerSlot = kInvalidPlayerSlot;
    uint32_t playerList = 0;
    uint32_t playerLookupKey = 0;
    uint32_t playerControlType = 0;
    uint32_t playerAi = 0;
    uint32_t playerSkirmishAi = 0;
    uint32_t playerListActiveCount = 0;
    uint32_t playerRawFlag106 = 0;
    uint32_t playerDefeated = 0;
    uint32_t playerIsCurrentOrLocal = 0;
    uint32_t playerActiveEligible = 0;

    uint32_t disabled = 0;
    uint32_t lastUpdateFrame = 0;
    int32_t timer = 0;
    int32_t strategicDirective = 0;
    uint32_t stateNextEvaluationFrame = 0;
    uint32_t strategyEnabled = 0;
    uint32_t strategyInterval = 0;
    uint32_t strategyNextFrame = 0;
    uint32_t personalityUpdateIntervalBits = 0;

    uint32_t compatibleFactionCount = 0;
    uint32_t compatibleFactions = 0;
    uint32_t weightedStructureCount = 0;
    uint32_t weightedStructures = 0;
    uint32_t strategicDefinitionCount = 0;
    uint32_t strategicDefinitions = 0;
    uint32_t budgetDefinitionCount = 0;
    uint32_t budgetDefinitions = 0;
    uint32_t buildDefinitionCount = 0;
    uint32_t buildDefinitions = 0;

    uint32_t stateBegin = 0;
    uint32_t stateEnd = 0;
    uint32_t stateCapacityEnd = 0;
    uint32_t stateTotalCount = 0;
    uint32_t stateCount = 0;
    uint32_t stateCapacityCount = 0;
    std::array<StrategicState, kMaxStrategicStates> states{};
};

struct PlaneSnapshot {
    uint32_t generation = 0;
    uint32_t skippedCaptures = 0;
    uint32_t agentSlotMask = 0;
    uint32_t rejectedIdentities = 0;
    uint64_t exportedUnixMs = 0;
    std::array<AgentSnapshot, kMaxAgents> agents{};
};

inline uint16_t read16(const uint8_t* p) {
    return uint16_t(p[0]) | uint16_t(p[1]) << 8;
}
inline uint32_t read32(const uint8_t* p) {
    return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 | uint32_t(p[3]) << 24;
}
inline void write16(uint8_t* p, uint16_t value) {
    p[0] = uint8_t(value); p[1] = uint8_t(value >> 8);
}
inline void write32(uint8_t* p, uint32_t value) {
    for(unsigned i = 0; i < 4; ++i) p[i] = uint8_t(value >> (8 * i));
}

inline RequestPacket encodeRequest(uint32_t sequence) {
    RequestPacket packet{};
    write32(packet.data(), kMagic);
    write16(packet.data() + 4, kVersion);
    write16(packet.data() + 6, uint16_t(Op::Snapshot));
    write32(packet.data() + 8, sequence);
    return packet;
}

inline bool decodeRequest(const RequestPacket& packet, Request& request) {
    if(read32(packet.data()) != kMagic || read16(packet.data() + 4) != kVersion ||
       read16(packet.data() + 6) != uint16_t(Op::Snapshot) || read32(packet.data() + 12) != 0) {
        return false;
    }
    request = {Op::Snapshot, read32(packet.data() + 8)};
    return true;
}

inline void append16(std::vector<uint8_t>& out, uint16_t value) {
    out.push_back(uint8_t(value)); out.push_back(uint8_t(value >> 8));
}
inline void append32(std::vector<uint8_t>& out, uint32_t value) {
    for(unsigned i = 0; i < 4; ++i) out.push_back(uint8_t(value >> (8 * i)));
}
inline void append64(std::vector<uint8_t>& out, uint64_t value) {
    for(unsigned i = 0; i < 8; ++i) out.push_back(uint8_t(value >> (8 * i)));
}
inline uint32_t countAgents(uint32_t slotMask) {
    uint32_t count = 0;
    for(slotMask &= kAllAgentSlotsMask; slotMask; slotMask &= slotMask - 1) ++count;
    return count;
}

inline std::vector<uint8_t> encodeResponse(const PlaneSnapshot& plane, uint32_t sequence, Status status) {
    std::vector<uint8_t> payload;
    const uint32_t slotMask = status == Status::Ok ? plane.agentSlotMask & kAllAgentSlotsMask : 0;
    const uint32_t agentCount = countAgents(slotMask);
    if(status == Status::Ok) {
        payload.reserve(size_t(agentCount) * (kAgentPrefixSize + kMaxStrategicStates * 12));
        for(uint32_t slot = 0; slot < kMaxAgents; ++slot) {
            if(!(slotMask & (1u << slot))) continue;
            const auto& a = plane.agents[slot];
            const size_t recordStart = payload.size();
            append32(payload, 0); // record size, patched after the variable state list
            append32(payload, a.flags);
            append32(payload, a.captureSequence);
            append32(payload, a.gameFrame);
            append64(payload, a.capturedUnixMs);
            for(uint32_t value : {a.skirmishAi, a.bank, a.buildManager, a.buildStateManager,
                                  a.strategicStateManager, a.strategyManager, a.economyManager,
                                  a.player, a.armyDefinition, a.personalityRef, a.personalityDefinition}) {
                append32(payload, value);
            }
            for(uint32_t value : {a.playerSlot, a.playerList, a.playerLookupKey,
                                  a.playerControlType, a.playerAi, a.playerSkirmishAi,
                                  a.playerListActiveCount, a.playerRawFlag106, a.playerDefeated,
                                  a.playerIsCurrentOrLocal, a.playerActiveEligible}) {
                append32(payload, value);
            }
            for(uint32_t value : {a.disabled, a.lastUpdateFrame, uint32_t(a.timer),
                                  uint32_t(a.strategicDirective), a.stateNextEvaluationFrame,
                                  a.strategyEnabled, a.strategyInterval, a.strategyNextFrame,
                                  a.personalityUpdateIntervalBits, a.compatibleFactionCount,
                                  a.compatibleFactions, a.weightedStructureCount, a.weightedStructures,
                                  a.strategicDefinitionCount, a.strategicDefinitions,
                                  a.budgetDefinitionCount, a.budgetDefinitions,
                                  a.buildDefinitionCount, a.buildDefinitions, a.stateBegin, a.stateEnd,
                                  a.stateCapacityEnd, a.stateTotalCount, a.stateCount,
                                  a.stateCapacityCount}) {
                append32(payload, value);
            }
            const uint32_t count = a.stateCount < kMaxStrategicStates ? a.stateCount : uint32_t(kMaxStrategicStates);
            for(uint32_t state = 0; state < count; ++state) {
                append32(payload, a.states[state].definition);
                append32(payload, a.states[state].scoreBits);
                append32(payload, a.states[state].flags);
            }
            write32(payload.data() + recordStart, uint32_t(payload.size() - recordStart));
        }
    }

    std::vector<uint8_t> response(kResponseHeaderSize, 0);
    write32(response.data(), kMagic);
    write16(response.data() + 4, kVersion);
    write16(response.data() + 6, uint16_t(status));
    write32(response.data() + 8, sequence);
    write32(response.data() + 12, uint32_t(payload.size()));
    write32(response.data() + 16, plane.generation);
    write32(response.data() + 20, agentCount);
    uint64_t timestamp = plane.exportedUnixMs;
    for(unsigned i = 0; i < 8; ++i) response[24 + i] = uint8_t(timestamp >> (8 * i));
    write32(response.data() + 32, plane.skippedCaptures);
    write32(response.data() + 36, slotMask);
    write32(response.data() + 40, plane.rejectedIdentities);
    response.insert(response.end(), payload.begin(), payload.end());
    return response;
}

} // namespace ra3::ai


#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <initializer_list>
#include <vector>

namespace ra3::ai {

constexpr uint32_t kMagic = 0x49413352; // bytes: R3AI
constexpr uint16_t kVersion = 3;
constexpr size_t kRequestSize = 16;
constexpr size_t kResponseHeaderSize = 48;
constexpr size_t kAgentPrefixSize = 540;
constexpr size_t kMaxAgents = 20;
constexpr size_t kMaxStrategicStates = 64;
constexpr size_t kMaxBudgetStates = 16;
constexpr size_t kMaxBuildStates = 96;
constexpr size_t kMaxTargets = 32;
constexpr size_t kMaxTactics = 64;
constexpr size_t kBankAccountCount = 6;
constexpr size_t kBankAccountWordCount = 9;
constexpr size_t kTacticWordCount = 12;
constexpr uint32_t kInvalidPlayerSlot = 0xffffffffu;
constexpr uint32_t kAllAgentSlotsMask = (1u << kMaxAgents) - 1u;

using RequestPacket = std::array<uint8_t, kRequestSize>;

enum class Op : uint16_t { Snapshot = 0 };
enum class Status : uint16_t { Ok = 0, Invalid = 1, NotReady = 2 };

struct Request {
    Op op = Op::Snapshot;
    uint32_t sequence = 0;
};

struct StateEntry {
    uint32_t wrapper = 0;
    uint32_t scoreBits = 0;
    uint32_t flags = 0;
    uint32_t definition = 0;
    uint32_t assetId = 0;
    uint32_t assetTypeId = 0;
    uint32_t assetInstanceId = 0;
};

struct BankAccount {
    std::array<uint32_t, kBankAccountWordCount> words{};
};

struct TargetSnapshot {
    uint32_t record = 0;
    uint32_t stateWrapper = 0;
    uint32_t tactic = 0;
    uint32_t objectId = 0;
    uint32_t statusBits = 0;
    uint32_t stateSelectionFrame = 0;
    uint32_t stateDefinition = 0;
    uint32_t stateAssetId = 0;
    uint32_t stateAssetTypeId = 0;
    uint32_t stateAssetInstanceId = 0;
};

struct TacticSnapshot {
    uint32_t instance = 0;
    std::array<uint32_t, kTacticWordCount> words{};
    uint32_t vtableRva = 0;
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
    uint32_t imageBase = 0;

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
    uint32_t personalityAssetId = 0;
    uint32_t personalityAssetTypeId = 0;
    uint32_t personalityAssetInstanceId = 0;

    uint32_t stateBegin = 0;
    uint32_t stateEnd = 0;
    uint32_t stateCapacityEnd = 0;
    uint32_t stateTotalCount = 0;
    uint32_t stateCount = 0;
    uint32_t stateCapacityCount = 0;
    std::array<StateEntry, kMaxStrategicStates> states{};

    uint32_t budgetStateBegin = 0;
    uint32_t budgetStateEnd = 0;
    uint32_t budgetStateCapacityEnd = 0;
    uint32_t budgetStateTotalCount = 0;
    uint32_t budgetStateCount = 0;
    uint32_t budgetStateCapacityCount = 0;
    std::array<StateEntry, kMaxBudgetStates> budgetStates{};

    uint32_t buildStateBegin = 0;
    uint32_t buildStateEnd = 0;
    uint32_t buildStateCapacityEnd = 0;
    uint32_t buildStateTotalCount = 0;
    uint32_t buildStateCount = 0;
    uint32_t buildStateCapacityCount = 0;
    std::array<StateEntry, kMaxBuildStates> buildStates{};

    uint32_t currentBudget = 0;
    std::array<BankAccount, kBankAccountCount> bankAccounts{};

    uint32_t targetManager = 0;
    uint32_t targetBegin = 0;
    uint32_t targetTotalCount = 0;
    uint32_t targetCount = 0;
    std::array<TargetSnapshot, kMaxTargets> targets{};

    uint32_t tacticManager = 0;
    uint32_t tacticBegin = 0;
    uint32_t tacticEnd = 0;
    uint32_t tacticCapacityEnd = 0;
    uint32_t tacticTotalCount = 0;
    uint32_t tacticCount = 0;
    uint32_t tacticCapacityCount = 0;
    std::array<TacticSnapshot, kMaxTactics> tactics{};
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

inline void appendStateEntries(std::vector<uint8_t>& out, const StateEntry* states, uint32_t count) {
    for(uint32_t i = 0; i < count; ++i) {
        for(uint32_t value : {states[i].wrapper, states[i].scoreBits, states[i].flags,
                              states[i].definition, states[i].assetId, states[i].assetTypeId,
                              states[i].assetInstanceId}) append32(out, value);
    }
}

inline std::vector<uint8_t> encodeResponse(const PlaneSnapshot& plane, uint32_t sequence, Status status) {
    std::vector<uint8_t> payload;
    const uint32_t slotMask = status == Status::Ok ? plane.agentSlotMask & kAllAgentSlotsMask : 0;
    const uint32_t agentCount = countAgents(slotMask);
    if(status == Status::Ok) {
        for(uint32_t slot = 0; slot < kMaxAgents; ++slot) {
            if(!(slotMask & (1u << slot))) continue;
            const auto& a = plane.agents[slot];
            const size_t recordStart = payload.size();
            append32(payload, 0); // record size, patched after all variable sections
            append32(payload, a.flags);
            append32(payload, a.captureSequence);
            append32(payload, a.gameFrame);
            append64(payload, a.capturedUnixMs);
            for(uint32_t value : {a.skirmishAi, a.bank, a.buildManager, a.buildStateManager,
                                  a.strategicStateManager, a.strategyManager, a.economyManager,
                                  a.player, a.armyDefinition, a.personalityRef, a.personalityDefinition,
                                  a.imageBase}) append32(payload, value);
            for(uint32_t value : {a.playerSlot, a.playerList, a.playerLookupKey,
                                  a.playerControlType, a.playerAi, a.playerSkirmishAi,
                                  a.playerListActiveCount, a.playerRawFlag106, a.playerDefeated,
                                  a.playerIsCurrentOrLocal, a.playerActiveEligible}) append32(payload, value);
            for(uint32_t value : {a.disabled, a.lastUpdateFrame, uint32_t(a.timer),
                                  uint32_t(a.strategicDirective), a.stateNextEvaluationFrame,
                                  a.strategyEnabled, a.strategyInterval, a.strategyNextFrame,
                                  a.personalityUpdateIntervalBits}) append32(payload, value);
            for(uint32_t value : {a.compatibleFactionCount, a.compatibleFactions,
                                  a.weightedStructureCount, a.weightedStructures,
                                  a.strategicDefinitionCount, a.strategicDefinitions,
                                  a.budgetDefinitionCount, a.budgetDefinitions,
                                  a.buildDefinitionCount, a.buildDefinitions,
                                  a.personalityAssetId, a.personalityAssetTypeId,
                                  a.personalityAssetInstanceId}) append32(payload, value);
            for(uint32_t value : {a.stateBegin, a.stateEnd, a.stateCapacityEnd,
                                  a.stateTotalCount, a.stateCount, a.stateCapacityCount,
                                  a.budgetStateBegin, a.budgetStateEnd, a.budgetStateCapacityEnd,
                                  a.budgetStateTotalCount, a.budgetStateCount, a.budgetStateCapacityCount,
                                  a.buildStateBegin, a.buildStateEnd, a.buildStateCapacityEnd,
                                  a.buildStateTotalCount, a.buildStateCount, a.buildStateCapacityCount,
                                  a.currentBudget}) append32(payload, value);
            for(const auto& account : a.bankAccounts)
                for(uint32_t value : account.words) append32(payload, value);
            for(uint32_t value : {a.targetManager, a.targetBegin, a.targetTotalCount, a.targetCount,
                                  a.tacticManager, a.tacticBegin, a.tacticEnd, a.tacticCapacityEnd,
                                  a.tacticTotalCount, a.tacticCount, a.tacticCapacityCount}) append32(payload, value);

            appendStateEntries(payload, a.states.data(), a.stateCount);
            appendStateEntries(payload, a.budgetStates.data(), a.budgetStateCount);
            appendStateEntries(payload, a.buildStates.data(), a.buildStateCount);
            for(uint32_t i = 0; i < a.targetCount; ++i) {
                const auto& target = a.targets[i];
                for(uint32_t value : {target.record, target.stateWrapper, target.tactic,
                                      target.objectId, target.statusBits, target.stateSelectionFrame,
                                      target.stateDefinition, target.stateAssetId,
                                      target.stateAssetTypeId, target.stateAssetInstanceId}) append32(payload, value);
            }
            for(uint32_t i = 0; i < a.tacticCount; ++i) {
                append32(payload, a.tactics[i].instance);
                for(uint32_t value : a.tactics[i].words) append32(payload, value);
                append32(payload, a.tactics[i].vtableRva);
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

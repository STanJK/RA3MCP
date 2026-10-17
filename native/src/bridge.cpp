#include <windows.h>
#include <bcrypt.h>
#include <mmsystem.h>
#include <MinHook.h>
#include "ai_plane_protocol.hpp"
#include "protocol.hpp"
#include "work_queue.hpp"
#include <array>
#include <atomic>
#include <cstdio>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

static_assert(sizeof(void*) == 4, "The game input bridge requires x86");

// One verified executable only. These are RVAs, never live absolute addresses.
namespace {
constexpr uintptr_t kIdaBase = 0x400000;
constexpr uintptr_t kPumpRva = 0x5416C0 - kIdaBase;
constexpr uintptr_t kMoveRva = 0x8B03F0 - kIdaBase;
constexpr uintptr_t kDownRva = 0x8B0450 - kIdaBase;
constexpr uintptr_t kUpRva = 0x8B04C0 - kIdaBase;
constexpr uintptr_t kWheelRva = 0x8B0530 - kIdaBase;
constexpr uintptr_t kWindowRva = 0xCD5C10 - kIdaBase;
constexpr uintptr_t kSkirmishUpdateRva = 0x909A90 - kIdaBase;
constexpr uintptr_t kGameLogicGlobalRva = 0xCDDE84 - kIdaBase;
constexpr char kSha256[] = "dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0";
using Pump = int (__thiscall*)(void*);
using SkirmishUpdate = int (__thiscall*)(void*);
using Move = int (__thiscall*)(void*, int, int, uint32_t);
using Button = int (__thiscall*)(void*, int, int, int, uint32_t);
using Wheel = int (__thiscall*)(void*, int, int, int);
Pump g_originalPump = nullptr;
SkirmishUpdate g_originalSkirmishUpdate = nullptr;
Move g_move = nullptr;
Button g_down = nullptr, g_up = nullptr;
Wheel g_wheel = nullptr;
uintptr_t g_base = 0;
HMODULE g_ownModule = nullptr;
std::atomic<bool> g_started{false}, g_releaseRequested{false};
std::atomic<uint32_t> g_pump{0};
std::atomic<uint8_t> g_held{0};
int g_x = 0, g_y = 0; // Only the game input thread touches these coordinates.

std::mutex g_planeMutex;
ra3::ai::PlaneSnapshot g_plane;
std::atomic<uint32_t> g_planeCaptureSequence{0};
std::atomic<uint32_t> g_planeSkippedCaptures{0};

enum class Phase { Queued, Executing, Done, Cancelled };
struct Request {
    ra3::Command command;
    HANDLE done = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    std::atomic<Phase> phase{Phase::Queued};
    ra3::Status status = ra3::Status::Pumped;
    uint32_t detail = 0, pump = 0;
    uint8_t held = 0;
    explicit Request(ra3::Command c) : command(c) {}
    ~Request() { if(done) CloseHandle(done); }
};
std::mutex g_queueMutex;
ra3::WorkQueue<std::shared_ptr<Request>, 64> g_queue;

std::wstring modulePath(HMODULE module) {
    std::vector<wchar_t> path(32768);
    const DWORD n = GetModuleFileNameW(module, path.data(), static_cast<DWORD>(path.size()));
    if(!n || n >= path.size()) return {};
    return {path.data(), n};
}
std::string utf8(const std::wstring& text) {
    if(text.empty()) return "<unavailable>";
    const int count = WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()),
                                          nullptr, 0, nullptr, nullptr);
    if(!count) return "<path conversion failed>";
    std::string result(static_cast<size_t>(count), '\0');
    if(!WideCharToMultiByte(CP_UTF8, 0, text.data(), static_cast<int>(text.size()),
                            result.data(), count, nullptr, nullptr)) return "<path conversion failed>";
    return result;
}
// Worker thread only. No file or pipe operations run in the input hook.
void log(const char* message) {
    const std::string line = "[pid=" + std::to_string(GetCurrentProcessId()) + "] " + message + "\r\n";
    OutputDebugStringA(line.c_str());
    auto path = modulePath(g_ownModule);
    const auto slash = path.find_last_of(L"\\/");
    if(slash == std::wstring::npos) return;
    path.resize(slash + 1);
    path += L"RA3InputBridge.log";
    HANDLE file = CreateFileW(path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                              nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if(file == INVALID_HANDLE_VALUE) return;
    DWORD written = 0;
    WriteFile(file, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
    CloseHandle(file);
}
void logWin32Error(const char* action, DWORD error) {
    char message[256]{};
    sprintf_s(message, "Refused: %s failed; Win32 error=%lu.", action, error);
    log(message);
}
bool checkHashStatus(const char* action, NTSTATUS status) {
    if(status >= 0) return true;
    char message[256]{};
    sprintf_s(message, "Refused: %s failed; NTSTATUS=0x%08lX.",
              action, static_cast<unsigned long>(status));
    log(message);
    return false;
}

bool checkExecutable(const std::wstring& path) {
    const auto dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(g_base);
    if(dos->e_magic != IMAGE_DOS_SIGNATURE || dos->e_lfanew <= 0) {
        log("Refused: invalid DOS/PE header in the host main module."); return false;
    }
    const auto nt = reinterpret_cast<const IMAGE_NT_HEADERS32*>(g_base + dos->e_lfanew);
    char image[192]{};
    sprintf_s(image, "Host image: machine=0x%04X magic=0x%04X size=0x%08lX.",
              static_cast<unsigned int>(nt->FileHeader.Machine),
              static_cast<unsigned int>(nt->OptionalHeader.Magic), nt->OptionalHeader.SizeOfImage);
    log(image);
    if(nt->Signature != IMAGE_NT_SIGNATURE || nt->FileHeader.Machine != IMAGE_FILE_MACHINE_I386 ||
       nt->OptionalHeader.Magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC) {
        log("Refused: host main module is not a valid PE32/x86 image."); return false;
    }
    if(nt->OptionalHeader.SizeOfImage <= kWindowRva + sizeof(HWND)) {
        log("Refused: host image is too small for the verified game RVAs."); return false;
    }
    if(path.empty()) {
        log("Refused: could not determine the host executable path."); return false;
    }
    HANDLE file = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                              OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, nullptr);
    if(file == INVALID_HANDLE_VALUE) {
        logWin32Error("CreateFileW(host executable)", GetLastError()); return false;
    }
    BCRYPT_ALG_HANDLE provider = nullptr;
    BCRYPT_HASH_HANDLE hash = nullptr;
    std::array<unsigned char, 32> digest{};
    bool ok = checkHashStatus("BCryptOpenAlgorithmProvider",
                               BCryptOpenAlgorithmProvider(&provider, BCRYPT_SHA256_ALGORITHM, nullptr, 0));
    if(ok) ok = checkHashStatus("BCryptCreateHash", BCryptCreateHash(provider, &hash, nullptr, 0, nullptr, 0, 0));
    std::array<unsigned char, 65536> bytes{};
    while(ok) {
        DWORD n = 0;
        if(!ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &n, nullptr)) {
            logWin32Error("ReadFile(host executable)", GetLastError()); ok = false; break;
        }
        if(!n) break;
        ok = checkHashStatus("BCryptHashData", BCryptHashData(hash, bytes.data(), n, 0));
    }
    if(ok) ok = checkHashStatus("BCryptFinishHash",
                                 BCryptFinishHash(hash, digest.data(), static_cast<ULONG>(digest.size()), 0));
    if(hash) BCryptDestroyHash(hash);
    if(provider) BCryptCloseAlgorithmProvider(provider, 0);
    CloseHandle(file);
    if(!ok) return false;
    constexpr char hex[] = "0123456789abcdef";
    std::array<char, 65> result{};
    for(size_t i = 0; i < digest.size(); ++i) {
        result[i * 2] = hex[digest[i] >> 4]; result[i * 2 + 1] = hex[digest[i] & 15];
    }
    log((std::string("Host SHA-256: ") + result.data()).c_str());
    log((std::string("Expected SHA-256: ") + kSha256).c_str());
    if(std::string(result.data()) != kSha256) {
        log("Refused: host executable SHA-256 does not match the verified ra3_1.12.game."); return false;
    }
    log("Verified game executable; installing input and AI data-plane hooks.");
    return true;
}

constexpr uint32_t kAgentSelfReadable = 1u << 0;
constexpr uint32_t kAgentGameClockReadable = 1u << 1;
constexpr uint32_t kAgentStateVectorValid = 1u << 2;
constexpr uint32_t kAgentStateVectorTruncated = 1u << 3;
constexpr uint32_t kAgentPersonalityReadable = 1u << 4;
constexpr uint32_t kAgentStrategyManagerReadable = 1u << 5;

uint64_t unixMilliseconds() {
    FILETIME time{};
    GetSystemTimeAsFileTime(&time);
    ULARGE_INTEGER value{};
    value.LowPart = time.dwLowDateTime;
    value.HighPart = time.dwHighDateTime;
    constexpr uint64_t kWindowsToUnix100ns = 116444736000000000ULL;
    return value.QuadPart >= kWindowsToUnix100ns ? (value.QuadPart - kWindowsToUnix100ns) / 10000ULL : 0;
}

bool readableRange(uintptr_t address, size_t size) {
    if(!address || !size || address + size < address) return false;
    const uintptr_t end = address + size;
    while(address < end) {
        MEMORY_BASIC_INFORMATION information{};
        if(!VirtualQuery(reinterpret_cast<const void*>(address), &information, sizeof(information)) ||
           information.State != MEM_COMMIT || information.Protect & (PAGE_GUARD | PAGE_NOACCESS)) {
            return false;
        }
        const DWORD protection = information.Protect & 0xff;
        if(protection != PAGE_READONLY && protection != PAGE_READWRITE && protection != PAGE_WRITECOPY &&
           protection != PAGE_EXECUTE_READ && protection != PAGE_EXECUTE_READWRITE &&
           protection != PAGE_EXECUTE_WRITECOPY) {
            return false;
        }
        const uintptr_t regionEnd = reinterpret_cast<uintptr_t>(information.BaseAddress) + information.RegionSize;
        if(regionEnd <= address) return false;
        address = regionEnd;
    }
    return true;
}

template<class T> T loadAt(uintptr_t address) {
    T value{};
    std::memcpy(&value, reinterpret_cast<const void*>(address), sizeof(value));
    return value;
}

ra3::ai::AgentSnapshot captureAgent(void* selfPointer) {
    ra3::ai::AgentSnapshot snapshot{};
    const uintptr_t self = reinterpret_cast<uintptr_t>(selfPointer);
    snapshot.captureSequence = g_planeCaptureSequence.fetch_add(1, std::memory_order_relaxed) + 1;
    snapshot.capturedUnixMs = unixMilliseconds();
    snapshot.skirmishAi = uint32_t(self);
    if(!readableRange(self, 0x914)) return snapshot;

    snapshot.flags |= kAgentSelfReadable;
    snapshot.bank = uint32_t(self + 0x004);
    snapshot.buildManager = uint32_t(self + 0x104);
    snapshot.buildStateManager = uint32_t(self + 0x108);
    snapshot.strategicStateManager = uint32_t(self + 0x6DC);
    snapshot.economyManager = uint32_t(self + 0x6FC);
    snapshot.strategyManager = loadAt<uint32_t>(self + 0x6F8);
    snapshot.player = loadAt<uint32_t>(self + 0x8A8);
    snapshot.armyDefinition = loadAt<uint32_t>(self + 0x8AC);
    snapshot.personalityRef = loadAt<uint32_t>(self + 0x8B0);
    snapshot.disabled = loadAt<uint8_t>(self + 0x8B4);
    snapshot.lastUpdateFrame = loadAt<uint32_t>(self + 0x8C0);
    snapshot.timer = loadAt<int32_t>(self + 0x8CC);
    snapshot.strategicDirective = loadAt<int32_t>(self + 0x8EC);
    snapshot.stateNextEvaluationFrame = loadAt<uint32_t>(self + 0x6F4);

    const uintptr_t gameLogicGlobal = g_base + kGameLogicGlobalRva;
    if(readableRange(gameLogicGlobal, sizeof(uint32_t))) {
        const uintptr_t gameLogic = loadAt<uint32_t>(gameLogicGlobal);
        if(readableRange(gameLogic, 0x54)) {
            snapshot.gameFrame = loadAt<uint32_t>(gameLogic + 0x50);
            snapshot.flags |= kAgentGameClockReadable;
        }
    }

    const uintptr_t personalityRef = snapshot.personalityRef;
    if(readableRange(personalityRef, sizeof(uint32_t))) {
        const uintptr_t definition = loadAt<uint32_t>(personalityRef);
        snapshot.personalityDefinition = uint32_t(definition);
        if(readableRange(definition, 0x16C)) {
            snapshot.flags |= kAgentPersonalityReadable;
            snapshot.personalityUpdateIntervalBits = loadAt<uint32_t>(definition + 0xE4);
            snapshot.compatibleFactionCount = loadAt<uint32_t>(definition + 0xEC);
            snapshot.compatibleFactions = loadAt<uint32_t>(definition + 0xF0);
            snapshot.weightedStructureCount = loadAt<uint32_t>(definition + 0x144);
            snapshot.weightedStructures = loadAt<uint32_t>(definition + 0x148);
            snapshot.strategicDefinitionCount = loadAt<uint32_t>(definition + 0x154);
            snapshot.strategicDefinitions = loadAt<uint32_t>(definition + 0x158);
            snapshot.budgetDefinitionCount = loadAt<uint32_t>(definition + 0x15C);
            snapshot.budgetDefinitions = loadAt<uint32_t>(definition + 0x160);
            snapshot.buildDefinitionCount = loadAt<uint32_t>(definition + 0x164);
            snapshot.buildDefinitions = loadAt<uint32_t>(definition + 0x168);
        }
    }

    const uintptr_t strategy = snapshot.strategyManager;
    if(readableRange(strategy, 0x1C)) {
        snapshot.flags |= kAgentStrategyManagerReadable;
        snapshot.strategyEnabled = loadAt<uint8_t>(strategy + 0x10);
        snapshot.strategyInterval = loadAt<uint32_t>(strategy + 0x14);
        snapshot.strategyNextFrame = loadAt<uint32_t>(strategy + 0x18);
    }

    const uintptr_t begin = loadAt<uint32_t>(self + 0x6E0);
    const uintptr_t end = loadAt<uint32_t>(self + 0x6E4);
    const uintptr_t capacityEnd = loadAt<uint32_t>(self + 0x6E8);
    snapshot.stateBegin = uint32_t(begin);
    snapshot.stateEnd = uint32_t(end);
    snapshot.stateCapacityEnd = uint32_t(capacityEnd);
    constexpr size_t kStateSize = 12;
    const bool empty = begin == 0 && end == 0 && capacityEnd == 0;
    if(empty || (begin && begin <= end && end <= capacityEnd &&
                 (end - begin) % kStateSize == 0 && (capacityEnd - begin) % kStateSize == 0)) {
        const size_t total = empty ? 0 : (end - begin) / kStateSize;
        const size_t capacity = empty ? 0 : (capacityEnd - begin) / kStateSize;
        const size_t copied = total < ra3::ai::kMaxStrategicStates ? total : ra3::ai::kMaxStrategicStates;
        if(!copied || readableRange(begin, copied * kStateSize)) {
            snapshot.flags |= kAgentStateVectorValid;
            snapshot.stateTotalCount = uint32_t(total);
            snapshot.stateCapacityCount = uint32_t(capacity);
            snapshot.stateCount = uint32_t(copied);
            if(copied < total) snapshot.flags |= kAgentStateVectorTruncated;
            for(size_t i = 0; i < copied; ++i) {
                const uintptr_t state = begin + i * kStateSize;
                snapshot.states[i].definition = loadAt<uint32_t>(state);
                snapshot.states[i].scoreBits = loadAt<uint32_t>(state + 4);
                snapshot.states[i].flags = loadAt<uint8_t>(state + 8);
            }
        }
    }
    return snapshot;
}

void publishAgent(const ra3::ai::AgentSnapshot& snapshot) {
    if(!g_planeMutex.try_lock()) {
        g_planeSkippedCaptures.fetch_add(1, std::memory_order_relaxed);
        return;
    }
    uint32_t slot = g_plane.agentCount;
    for(uint32_t i = 0; i < g_plane.agentCount; ++i) {
        if(g_plane.agents[i].skirmishAi == snapshot.skirmishAi) { slot = i; break; }
    }
    if(slot >= ra3::ai::kMaxAgents) {
        slot = 0;
        for(uint32_t i = 1; i < ra3::ai::kMaxAgents; ++i) {
            if(g_plane.agents[i].captureSequence < g_plane.agents[slot].captureSequence) slot = i;
        }
    } else if(slot == g_plane.agentCount) {
        ++g_plane.agentCount;
    }
    g_plane.agents[slot] = snapshot;
    ++g_plane.generation;
    g_plane.skippedCaptures = g_planeSkippedCaptures.load(std::memory_order_relaxed);
    g_planeMutex.unlock();
}

void releaseButtons(void* mouse) {
    const auto mask = g_held.load(std::memory_order_relaxed);
    for(int b = 0; b < 3; ++b)
        if(mask & (1u << b)) g_up(mouse, b, g_x, g_y, timeGetTime());
    g_held.store(0, std::memory_order_relaxed);
}

void execute(Request& request, void* mouse) {
    const auto& c = request.command;
    if(c.op == ra3::Op::Ping) return;
    if(c.op == ra3::Op::ReleaseAll) { releaseButtons(mouse); return; }
    const HWND window = *reinterpret_cast<const HWND*>(g_base + kWindowRva);
    RECT rect{};
    if(!window || !GetClientRect(window, &rect)) {
        request.status = ra3::Status::NotReady; request.detail = 1; return;
    }
    if(c.x >= rect.right || c.y >= rect.bottom) {
        request.status = ra3::Status::Invalid; request.detail = 2; return;
    }
    g_x = c.x; g_y = c.y;
    auto held = g_held.load(std::memory_order_relaxed);
    switch(c.op) {
    case ra3::Op::Move: g_move(mouse, c.x, c.y, timeGetTime()); break;
    case ra3::Op::Down:
        if(held & (1u << c.button)) { request.status = ra3::Status::Invalid; request.detail = 3; break; }
        g_down(mouse, c.button, c.x, c.y, timeGetTime());
        held = static_cast<uint8_t>(held | (1u << c.button)); break;
    case ra3::Op::Up:
        // Do not release a button the bridge never pressed.
        if(!(held & (1u << c.button))) { request.status = ra3::Status::Invalid; request.detail = 4; break; }
        g_up(mouse, c.button, c.x, c.y, timeGetTime());
        held = static_cast<uint8_t>(held & ~(1u << c.button)); break;
    case ra3::Op::Wheel: g_wheel(mouse, c.x, c.y, c.value); break;
    default: break;
    }
    g_held.store(held, std::memory_order_relaxed);
}

int __fastcall hookedPump(void* self, void*) {
    const uint32_t pump = g_pump.fetch_add(1, std::memory_order_relaxed) + 1;
    void* mouse = static_cast<unsigned char*>(self) + 0x38;
    if(g_releaseRequested.exchange(false, std::memory_order_acq_rel)) releaseButtons(mouse);
    std::array<std::shared_ptr<Request>, 32> pending{};
    size_t count = 0;
    // Never wait for the pipe thread's queue lock on the game thread.
    if(g_queueMutex.try_lock()) {
        std::shared_ptr<Request> item;
        while(count < pending.size() && g_queue.pop(item)) pending[count++] = std::move(item);
        g_queueMutex.unlock();
    }
    for(size_t i = 0; i < count; ++i) {
        auto expected = Phase::Queued;
        if(pending[i]->phase.compare_exchange_strong(expected, Phase::Executing))
            execute(*pending[i], mouse);
    }
    const int result = g_originalPump(self); // The original drains the injected mouse queue.
    for(size_t i = 0; i < count; ++i) {
        auto& r = *pending[i];
        if(r.phase.load() != Phase::Executing) continue;
        r.pump = pump; r.held = g_held.load(std::memory_order_relaxed);
        r.phase.store(Phase::Done, std::memory_order_release);
        SetEvent(r.done);
    }
    return result;
}

int __fastcall hookedSkirmishUpdate(void* self, void*) {
    const int result = g_originalSkirmishUpdate(self);
    // The game thread only copies verified fields into a bounded in-process plane.
    // A contended export lock drops this sample instead of waiting.
    publishAgent(captureAgent(self));
    return result;
}

bool readExact(HANDLE pipe, void* bytes, size_t size) {
    auto* data = static_cast<uint8_t*>(bytes);
    size_t offset = 0;
    while(offset < size) {
        DWORD n = 0;
        if(!ReadFile(pipe, data + offset, static_cast<DWORD>(size - offset), &n, nullptr) || !n) return false;
        offset += n;
    }
    return true;
}
bool writeExact(HANDLE pipe, const void* bytes, size_t size) {
    const auto* data = static_cast<const uint8_t*>(bytes);
    size_t offset = 0;
    while(offset < size) {
        DWORD n = 0;
        if(!WriteFile(pipe, data + offset, static_cast<DWORD>(size - offset), &n, nullptr) || !n) return false;
        offset += n;
    }
    return true;
}
bool readPacket(HANDLE pipe, ra3::Packet& packet) { return readExact(pipe, packet.data(), packet.size()); }
bool writePacket(HANDLE pipe, const ra3::Packet& packet) { return writeExact(pipe, packet.data(), packet.size()); }
ra3::Packet dispatch(const ra3::Packet& packet) {
    ra3::Command command;
    const auto sequence = ra3::read32(packet.data() + 8);
    const auto failure = [sequence](ra3::Status status, uint32_t detail = 0) {
        return ra3::ack(sequence, status, g_pump.load(), g_held.load(), detail);
    };
    if(!ra3::decode(packet, command)) return failure(ra3::Status::Invalid);
    auto request = std::make_shared<Request>(command);
    if(!request->done) return failure(ra3::Status::NotReady, GetLastError());
    {
        std::lock_guard<std::mutex> guard(g_queueMutex);
        if(!g_queue.push(request)) return failure(ra3::Status::Busy);
    }
    const DWORD wait = WaitForSingleObject(request->done, 3000);
    if(request->phase.load(std::memory_order_acquire) == Phase::Done)
        return ra3::ack(sequence, request->status, request->pump, request->held, request->detail);
    auto expected = Phase::Queued;
    const bool cancelled = request->phase.compare_exchange_strong(expected, Phase::Cancelled);
    // A timeout during execution has an unknown outcome. Release on the next pump;
    // the controller must not automatically retry clicks or drags.
    g_releaseRequested.store(true, std::memory_order_release);
    return failure(ra3::Status::Timeout, wait == WAIT_FAILED ? GetLastError() : (cancelled ? 1u : 2u));
}

std::vector<uint8_t> planeDispatch(const ra3::ai::RequestPacket& packet) {
    ra3::ai::Request request{};
    const uint32_t sequence = ra3::ai::read32(packet.data() + 8);
    if(!ra3::ai::decodeRequest(packet, request)) {
        ra3::ai::PlaneSnapshot empty{};
        empty.exportedUnixMs = unixMilliseconds();
        return ra3::ai::encodeResponse(empty, sequence, ra3::ai::Status::Invalid);
    }
    ra3::ai::PlaneSnapshot snapshot{};
    {
        std::lock_guard<std::mutex> guard(g_planeMutex);
        snapshot = g_plane;
    }
    snapshot.skippedCaptures = g_planeSkippedCaptures.load(std::memory_order_relaxed);
    snapshot.exportedUnixMs = unixMilliseconds();
    const auto status = snapshot.agentCount ? ra3::ai::Status::Ok : ra3::ai::Status::NotReady;
    return ra3::ai::encodeResponse(snapshot, request.sequence, status);
}

DWORD WINAPI servePlane(void*) {
    const std::wstring name = L"\\\\.\\pipe\\RA3MCP.AI." + std::to_wstring(GetCurrentProcessId());
    char message[160]{};
    sprintf_s(message, "AI data plane ready: \\\\.\\pipe\\RA3MCP.AI.%lu (snapshot protocol v1)",
              GetCurrentProcessId());
    bool announced = false;
    for(;;) {
        HANDLE pipe = CreateNamedPipeW(name.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1, 65536, 4096, 0, nullptr);
        if(pipe == INVALID_HANDLE_VALUE) { log("AI data-plane pipe creation failed; restart the game."); return 1; }
        if(!announced) { log(message); announced = true; }
        if(ConnectNamedPipe(pipe, nullptr) || GetLastError() == ERROR_PIPE_CONNECTED) {
            ra3::ai::RequestPacket packet{};
            while(readExact(pipe, packet.data(), packet.size())) {
                const auto response = planeDispatch(packet);
                if(!writeExact(pipe, response.data(), response.size())) break;
            }
        }
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
    }
}

void serve() {
    const std::wstring name = L"\\\\.\\pipe\\RA3MCP." + std::to_wstring(GetCurrentProcessId());
    char message[128]{};
    sprintf_s(message, "Ready: \\\\.\\pipe\\RA3MCP.%lu (24-byte protocol v1)", GetCurrentProcessId());
    bool announced = false;
    for(;;) {
        HANDLE pipe = CreateNamedPipeW(name.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1, 4096, 4096, 0, nullptr);
        if(pipe == INVALID_HANDLE_VALUE) { log("Pipe creation failed; restart the game after checking the pipe name."); return; }
        try {
            if(!announced) { log(message); announced = true; }
            if(ConnectNamedPipe(pipe, nullptr) || GetLastError() == ERROR_PIPE_CONNECTED) {
                ra3::Packet packet{};
                while(readPacket(pipe, packet)) {
                    if(!writePacket(pipe, dispatch(packet))) break;
                }
            }
        } catch(...) {
            g_releaseRequested.store(true, std::memory_order_release);
            DisconnectNamedPipe(pipe);
            CloseHandle(pipe);
            throw;
        }
        g_releaseRequested.store(true, std::memory_order_release);
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
    }
}
DWORD WINAPI bootstrap(void*) {
    try {
        g_base = reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
        const auto host = modulePath(nullptr);
        log("Starting RA3InputBridge with host diagnostics.");
        log((std::string("Host executable: ") + utf8(host)).c_str());
        log((std::string("Bridge module: ") + utf8(modulePath(g_ownModule))).c_str());
        const auto slash = host.find_last_of(L"\\/");
        const auto name = host.substr(slash == std::wstring::npos ? 0 : slash + 1);
        // UAL may load this ASI in both the launcher and its game child.
        // Never apply game RVAs to RA3.exe. Each process initializes independently.
        if(lstrcmpiW(name.c_str(), L"RA3.exe") == 0) {
            log("Skipped launcher RA3.exe; no hooks or pipe installed in this process.");
            return 0;
        }
        if(!checkExecutable(host)) return 1;
        g_move = reinterpret_cast<Move>(g_base + kMoveRva);
        g_down = reinterpret_cast<Button>(g_base + kDownRva);
        g_up = reinterpret_cast<Button>(g_base + kUpRva);
        g_wheel = reinterpret_cast<Wheel>(g_base + kWheelRva);
        auto pumpTarget = reinterpret_cast<LPVOID>(g_base + kPumpRva);
        auto skirmishTarget = reinterpret_cast<LPVOID>(g_base + kSkirmishUpdateRva);
        if(MH_Initialize() != MH_OK) { log("MinHook initialization failed."); return 2; }
        if(MH_CreateHook(pumpTarget, reinterpret_cast<LPVOID>(&hookedPump),
                         reinterpret_cast<LPVOID*>(&g_originalPump)) != MH_OK) {
            log("Input pump hook creation failed."); MH_Uninitialize(); return 3;
        }
        if(MH_CreateHook(skirmishTarget, reinterpret_cast<LPVOID>(&hookedSkirmishUpdate),
                         reinterpret_cast<LPVOID*>(&g_originalSkirmishUpdate)) != MH_OK) {
            log("AI data-plane hook creation failed.");
            MH_RemoveHook(pumpTarget); MH_Uninitialize(); return 4;
        }
        if(MH_EnableHook(pumpTarget) != MH_OK) {
            log("Input pump hook enable failed.");
            MH_RemoveHook(skirmishTarget); MH_RemoveHook(pumpTarget); MH_Uninitialize(); return 5;
        }
        if(MH_EnableHook(skirmishTarget) != MH_OK) {
            log("AI data-plane hook enable failed.");
            MH_DisableHook(pumpTarget); MH_RemoveHook(skirmishTarget); MH_RemoveHook(pumpTarget);
            MH_Uninitialize(); return 6;
        }
        HANDLE planeThread = CreateThread(nullptr, 0, &servePlane, nullptr, 0, nullptr);
        if(planeThread) CloseHandle(planeThread);
        else log("AI data-plane worker creation failed; mouse input bridge remains available.");
        // Resident until process exit. Even a later pipe failure must not unmap the hook.
        serve();
    } catch(...) { log("Bridge worker failed; restart the game before retrying."); }
    g_releaseRequested.store(true, std::memory_order_release);
    return 0;
}
}

// UAL calls this after loading the ASI when DontLoadFromDllMain=1.
// Export spelling is fixed by bridge.def for MSVC x86.
extern "C" void __cdecl InitializeASI() {
    if(g_started.exchange(true)) return;
    if(!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
        reinterpret_cast<LPCWSTR>(&InitializeASI), &g_ownModule)) return;
    HANDLE thread = CreateThread(nullptr, 0, &bootstrap, nullptr, 0, nullptr);
    if(thread) CloseHandle(thread);
    else OutputDebugStringA("RA3InputBridge: worker thread creation failed.\n");
}
BOOL WINAPI DllMain(HINSTANCE, DWORD, LPVOID) { return TRUE; }

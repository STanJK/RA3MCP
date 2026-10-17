#pragma once
#include <array>
#include <cstddef>
#include <cstdint>

namespace ra3 {
constexpr uint32_t kMagic = 0x504D3352; // bytes: R3MP
constexpr uint16_t kVersion = 1;
constexpr size_t kPacketSize = 24;
using Packet = std::array<uint8_t, kPacketSize>;
enum class Op : uint8_t { Ping=0, Move=1, Down=2, Up=3, Wheel=4, ReleaseAll=5 };
enum class Status : uint8_t { Pumped=0, Invalid=1, NotReady=2, Busy=3, Timeout=4 };
struct Command {
    Op op = Op::Ping;
    uint8_t button = 0;
    uint32_t sequence = 0;
    int32_t x = 0, y = 0, value = 0;
};
inline uint16_t read16(const uint8_t* p) { return uint16_t(p[0]) | uint16_t(p[1]) << 8; }
inline uint32_t read32(const uint8_t* p) {
    return uint32_t(p[0]) | uint32_t(p[1])<<8 | uint32_t(p[2])<<16 | uint32_t(p[3])<<24;
}
inline void write16(uint8_t* p,uint16_t v) { p[0]=uint8_t(v); p[1]=uint8_t(v>>8); }
inline void write32(uint8_t* p,uint32_t v) {
    for(unsigned i=0;i<4;++i) p[i]=uint8_t(v>>(8*i));
}
inline Packet encode(const Command& c) {
    Packet p{}; write32(p.data(),kMagic); write16(p.data()+4,kVersion);
    p[6]=uint8_t(c.op); p[7]=c.button; write32(p.data()+8,c.sequence);
    write32(p.data()+12,uint32_t(c.x)); write32(p.data()+16,uint32_t(c.y));
    write32(p.data()+20,uint32_t(c.value)); return p;
}
inline bool decode(const Packet& p, Command& c) {
    if(read32(p.data())!=kMagic || read16(p.data()+4)!=kVersion || p[6]>uint8_t(Op::ReleaseAll)) return false;
    c={Op(p[6]),p[7],read32(p.data()+8),int32_t(read32(p.data()+12)),
       int32_t(read32(p.data()+16)),int32_t(read32(p.data()+20))};
    if((c.op==Op::Down || c.op==Op::Up) && c.button>2) return false;
    if(c.op==Op::Move || c.op==Op::Down || c.op==Op::Up || c.op==Op::Wheel) {
        if(c.x<0 || c.y<0 || c.x>65535 || c.y>65535) return false;
    }
    if(c.op==Op::Wheel && (c.value==0 || c.value < -100 || c.value>100)) return false;
    return true;
}
inline Packet ack(uint32_t sequence, Status status, uint32_t pump, uint8_t held, uint32_t detail=0) {
    Packet p{}; write32(p.data(),kMagic); write16(p.data()+4,kVersion);
    p[6]=uint8_t(status); p[7]=held; write32(p.data()+8,sequence);
    write32(p.data()+12,pump); write32(p.data()+16,detail); return p;
}
}

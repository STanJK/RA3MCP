#include "protocol.hpp"
#include "work_queue.hpp"
#include <cstdlib>
#include <iostream>
#include <memory>

static void check(bool ok) { if(!ok) { std::cerr<<"core test failed\n"; std::exit(1); } }
int main() {
    using namespace ra3;
    // Golden packet independently fixes byte order, field offsets, and signed wheel value.
    const Packet golden={0x52,0x33,0x4d,0x50,1,0,4,0,0x78,0x56,0x34,0x12,
        0x20,3,0,0,0x58,2,0,0,0xfe,0xff,0xff,0xff};
    Command c{}; check(decode(golden,c));
    check(c.op==Op::Wheel && c.sequence==0x12345678 && c.x==800 && c.y==600 && c.value==-2);
    check(encode(c)==golden);
    for(size_t offset: {size_t(0),size_t(4),size_t(6)}) {
        auto bad=golden; bad[offset]=255; check(!decode(bad,c));
    }
    auto bad=encode({Op::Down,3,1,1,1,0}); check(!decode(bad,c));
    bad=encode({Op::Move,0,1,-1,1,0}); check(!decode(bad,c));
    bad=encode({Op::Wheel,0,1,1,1,101}); check(!decode(bad,c));
    auto reply=ack(0x12345678,Status::Busy,42,5,9);
    check(reply[6]==3 && reply[7]==5 && read32(reply.data()+12)==42 && read32(reply.data()+16)==9);
    WorkQueue<std::shared_ptr<int>,2> q;
    auto a=std::make_shared<int>(7),b=std::make_shared<int>(8);
    check(q.push(a)); check(q.push(b)); check(!q.push(a));
    std::shared_ptr<int> v; check(q.pop(v)&&*v==7); check(q.push(a));
    check(q.pop(v)&&*v==8); check(q.pop(v)&&*v==7); check(!q.pop(v));
    std::cout<<"protocol golden/validation and bounded FIFO tests passed\n";
}

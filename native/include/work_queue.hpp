#pragma once
#include <array>
#include <cstddef>
#include <utility>

namespace ra3 {
// Callers own synchronization. The queue never allocates or overwrites old work.
template<class T, size_t Capacity> class WorkQueue {
    static_assert(Capacity>0);
    std::array<T,Capacity> slots_{};
    size_t first_=0, count_=0;
public:
    bool push(T value) {
        if(count_==Capacity) return false;
        slots_[(first_+count_)%Capacity]=std::move(value); ++count_; return true;
    }
    bool pop(T& value) {
        if(!count_) return false;
        value=std::move(slots_[first_]); slots_[first_]=T{};
        first_=(first_+1)%Capacity; --count_; return true;
    }
    size_t size() const { return count_; }
};
}

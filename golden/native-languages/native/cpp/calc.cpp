#include <jni.h>
#include <array>
#include <cstdint>
namespace workbench {
template<std::uint32_t Seed> class Mixer {
    std::uint32_t state=2166136261u ^ Seed;
public:
    void feed(std::uint32_t value) {
        std::array<unsigned char,4> bytes{};
        for (unsigned i=0;i<bytes.size();++i) bytes[i]=static_cast<unsigned char>(value>>(8*i));
        for (auto b:bytes) state=(state ^ b)*16777619u;
    }
    std::uint32_t result() const {return state&0x7fffffffu;}
};
}
extern "C" JNIEXPORT jint JNICALL Java_org_rustydroid_nativeworkbench_MainActivity_digest(JNIEnv*,jclass,jint a,jint b) {
    workbench::Mixer<2> mixer;
    mixer.feed(static_cast<std::uint32_t>(a));mixer.feed(static_cast<std::uint32_t>(b));
    return static_cast<jint>(mixer.result());
}
extern "C" JNIEXPORT jint JNICALL Java_org_rustydroid_nativeworkbench_MainActivity_nativeFlavor(JNIEnv*,jclass) {return 2;}

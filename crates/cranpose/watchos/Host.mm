#import "Host.h"
#import <Foundation/Foundation.h>
#include "cranpose-watchos-runner/src/lib.rs.h"
#include "rust/cxx.h"
#include <memory>
#include <string>

static std::unique_ptr<rust::Box<cranpose_watchos::Application>> app;
static std::string error;
static bool active = true;

bool CPResize(uint32_t width, uint32_t height, float density) {
    try {
        if (app) {
            (*app)->resize(width, height, density);
        } else {
            app = std::make_unique<rust::Box<cranpose_watchos::Application>>(
                cranpose_watchos::create_application(width, height, density));
            (*app)->set_active(active);
        }
        return true;
    } catch (const rust::Error &failure) {
        error = failure.what();
        return false;
    }
}

CGImageRef CPFrame(uint64_t elapsed) {
    if (!app || !(*app)->tick(elapsed)) return nullptr;
    auto pixels = (*app)->pixels();
    CFDataRef data = CFDataCreate(kCFAllocatorDefault, pixels.data(), pixels.size());
    if (!data) return nullptr;
    CGDataProviderRef provider = CGDataProviderCreateWithCFData(data);
    CFRelease(data);
    if (!provider) return nullptr;
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGImageRef image = CGImageCreate((*app)->width(), (*app)->height(), 8, 32,
        (*app)->width() * 4, space, kCGImageAlphaLast | kCGBitmapByteOrder32Big,
        provider, nullptr, false, kCGRenderingIntentDefault);
    CGColorSpaceRelease(space);
    CGDataProviderRelease(provider);
    return image;
}

void CPTouch(uint8_t phase, float x, float y) { if (app) (*app)->touch(phase, x, y); }
void CPCrown(float delta, uint64_t elapsed) { if (app) (*app)->crown(delta, elapsed); }
void CPActive(bool value) { active = value; if (app) (*app)->set_active(value); }
void CPSafeArea(float left, float top, float right, float bottom) {
    if (app) (*app)->set_safe_area(left, top, right, bottom);
}
const char *CPError(void) { return error.c_str(); }

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

bool CPTick(uint64_t elapsed) { return app && (*app)->tick(elapsed); }

const float *CPSprites(size_t *count) {
    *count = 0;
    if (!app) return nullptr;
    auto sprites = (*app)->sprites();
    *count = sprites.size();
    return sprites.data();
}

size_t CPUploadCount(void) { return app ? (*app)->upload_count() : 0; }

const uint8_t *CPUpload(size_t index, uint32_t *id, uint32_t *width, uint32_t *height) {
    *id = 0;
    *width = 0;
    *height = 0;
    if (!app) return nullptr;
    *id = (*app)->upload_id(index);
    *width = (*app)->upload_width(index);
    *height = (*app)->upload_height(index);
    return (*app)->upload_pixels(index).data();
}

size_t CPReleaseCount(void) { return app ? (*app)->releases().size() : 0; }

uint32_t CPRelease(size_t index) {
    if (!app) return 0;
    auto releases = (*app)->releases();
    return index < releases.size() ? releases[index] : 0;
}

void CPTouch(uint8_t phase, float x, float y) { if (app) (*app)->touch(phase, x, y); }
void CPCrown(float delta, uint64_t elapsed) { if (app) (*app)->crown(delta, elapsed); }
void CPActive(bool value) { active = value; if (app) (*app)->set_active(value); }
void CPSafeArea(float left, float top, float right, float bottom) {
    if (app) (*app)->set_safe_area(left, top, right, bottom);
}
const char *CPError(void) { return error.c_str(); }

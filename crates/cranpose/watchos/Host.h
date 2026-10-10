#pragma once
#include <CoreGraphics/CoreGraphics.h>
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
bool CPResize(uint32_t width, uint32_t height, float density);
CGImageRef _Nullable CPFrame(uint64_t elapsed) CF_RETURNS_RETAINED;
void CPTouch(uint8_t phase, float x, float y);
void CPCrown(float delta, uint64_t elapsed);
void CPActive(bool active);
void CPSafeArea(float left, float top, float right, float bottom);
const char * _Nonnull CPError(void);
#ifdef __cplusplus
}
#endif

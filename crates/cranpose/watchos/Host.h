#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
bool CPResize(uint32_t width, uint32_t height, float density);
/// Advances the frame clock; whether the picture changed.
bool CPTick(uint64_t elapsed);
/// The picture as sprites: `*count` floats, `SPRITE_FLOATS` per sprite.
const float * _Nullable CPSprites(size_t * _Nonnull count);
/// The textures the last `CPSprites` first used: premultiplied RGBA8, top row first.
size_t CPUploadCount(void);
const uint8_t * _Nullable CPUpload(size_t index, uint32_t * _Nonnull id, uint32_t * _Nonnull width,
                                   uint32_t * _Nonnull height);
/// The textures the last `CPSprites` no longer uses.
size_t CPReleaseCount(void);
uint32_t CPRelease(size_t index);
void CPTouch(uint8_t phase, float x, float y);
void CPCrown(float delta, uint64_t elapsed);
void CPActive(bool active);
void CPSafeArea(float left, float top, float right, float bottom);
const char * _Nonnull CPError(void);
#ifdef __cplusplus
}
#endif

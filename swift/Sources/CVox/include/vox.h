#ifndef VOX_H
#define VOX_H

#pragma once

/* Generated with cbindgen:0.29.2 */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define VOX_STATUS_OK 0

#define VOX_STATUS_NULL_POINTER 1

#define VOX_STATUS_SCHEMA 2

typedef struct VoxByteBuffer {
  uint8_t *ptr;
  size_t len;
  size_t cap;
} VoxByteBuffer;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

void vox_byte_buffer_free(struct VoxByteBuffer buffer);

int32_t vox_schema_payload_from_binette_schema_bundle(const uint8_t *schema_bundle_ptr,
                                                      size_t schema_bundle_len,
                                                      struct VoxByteBuffer *out);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* VOX_H */

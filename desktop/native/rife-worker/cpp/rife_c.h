// C-linkage shim over the VS-RIFE C++ RIFE class.
// Exposes a minimal opaque-handle API so that Rust can call RIFE without
// depending on C++ name mangling.

#pragma once

#include <stddef.h>  // ptrdiff_t

#ifdef __cplusplus
extern "C" {
#endif

/// Opaque handle to a RIFE inference instance.
typedef void* RifeHandle;

/// Initialize the Vulkan/ncnn GPU subsystem. Must be called before any other
/// rife_* function. Returns 0 on success, non-zero on failure.
int rife_init_gpu(void);

/// Tear down the Vulkan/ncnn GPU subsystem. Call after all handles have been
/// destroyed.
void rife_cleanup_gpu(void);

/// Number of Vulkan-capable GPUs.
int rife_gpu_count(void);

/// Index of the GPU ncnn prefers (a discrete GPU when present).
int rife_default_gpu(void);

/// Device name of GPU `gpuid`, or NULL when out of range. Owned by ncnn.
const char* rife_gpu_name(int gpuid);

/// Create a RIFE instance on GPU `gpuid` (negative = rife_default_gpu()) and
/// load the model in `model_dir` (absolute, UTF-8).
///
/// The model directory name selects the RIFE generation and padding, matching
/// the VapourSynth plugin.
///
/// @return Non-null handle on success, NULL on failure.
RifeHandle rife_create(int gpuid, const char* model_dir);

/// Interpolate the frame at `timestep` (0..1) between `src0` and `src1`.
///
/// Planes are planar float32 RGB in 0..1. `stride` is the row stride in
/// *elements* (floats), not bytes; for tightly packed planes it equals `w`.
///
/// @return 0 on success, non-zero on failure.
int rife_process_v4(
    RifeHandle       handle,
    const float*     src0R, const float* src0G, const float* src0B,
    const float*     src1R, const float* src1G, const float* src1B,
    float*           dstR,  float* dstG,        float* dstB,
    int w, int h, ptrdiff_t stride,
    float timestep);

/// Destroy a handle returned by rife_create.
void rife_destroy(RifeHandle handle);

#ifdef __cplusplus
}
#endif

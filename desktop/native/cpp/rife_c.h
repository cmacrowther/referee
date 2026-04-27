// C-linkage shim over the VS-RIFE C++ RIFE class.
// Exposes a minimal opaque-handle API so that Rust (via rife_ffi.rs) can call
// RIFE without depending on C++ name mangling or needing RTTI / exceptions.

#pragma once

#include <stddef.h>  // ptrdiff_t

#ifdef __cplusplus
extern "C" {
#endif

/// Opaque handle to a RIFE inference instance.
typedef void* RifeHandle;

/// Create a new RIFE instance and load the model from `model_dir_utf8`.
///
/// The function inspects the model directory name to determine the correct
/// `rife_v4` flag and `padding` value (matching the VapourSynth plugin logic).
///
/// @param gpuid       Vulkan device index (0 = first GPU).
/// @param model_dir   Absolute path to the model directory, UTF-8 encoded.
///
/// @return Non-null handle on success, NULL on failure.
RifeHandle rife_create(int gpuid, const char* model_dir);

/// Interpolate one frame between `src0` and `src1`.
///
/// All image planes are planar float32 (row-major, no padding between rows).
/// `stride` is the row stride in *bytes* (typically `w * sizeof(float)`).
/// `timestep` is usually 0.5 for a mid-point interpolation.
///
/// @return 0 on success, non-zero on failure.
int rife_process_v4(
    RifeHandle       handle,
    const float*     src0R, const float* src0G, const float* src0B,
    const float*     src1R, const float* src1G, const float* src1B,
    float*           dstR,  float* dstG,        float* dstB,
    int w, int h, ptrdiff_t stride,
    float timestep);

/// Destroy and free a RIFE handle previously returned by `rife_create`.
void rife_destroy(RifeHandle handle);

/// Return the number of Vulkan-capable GPU devices on this machine.
int rife_gpu_count(void);

/// Initialize the Vulkan/ncnn GPU subsystem.  Must be called before any
/// other rife_* function.  Returns 0 on success, non-zero on failure.
int rife_init_gpu(void);

/// Tear down the Vulkan/ncnn GPU subsystem.  Call after all handles have
/// been destroyed.
void rife_cleanup_gpu(void);

#ifdef __cplusplus
}
#endif

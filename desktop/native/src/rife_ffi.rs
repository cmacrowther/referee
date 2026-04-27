// Safe Rust wrappers around the rife_c.h C shim.
//
// Only compiled when the `rife-ffi` feature is active (i.e. when building the
// `rife-worker` binary target).  The rest of the crate never links against
// ncnn.

#![allow(dead_code)]

use std::ffi::CString;

// ---------------------------------------------------------------------------
// Raw FFI declarations
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct RifeHandleRaw(std::ffi::c_void);

extern "C" {
    fn rife_init_gpu() -> std::ffi::c_int;
    fn rife_cleanup_gpu();
    fn rife_gpu_count() -> std::ffi::c_int;
    fn rife_create(gpuid: std::ffi::c_int, model_dir: *const std::ffi::c_char) -> *mut RifeHandleRaw;
    fn rife_process_v4(
        handle: *mut RifeHandleRaw,
        src0_r: *const f32, src0_g: *const f32, src0_b: *const f32,
        src1_r: *const f32, src1_g: *const f32, src1_b: *const f32,
        dst_r:  *mut f32,   dst_g:  *mut f32,   dst_b:  *mut f32,
        w: std::ffi::c_int, h: std::ffi::c_int, stride: isize,
        timestep: f32,
    ) -> std::ffi::c_int;
    fn rife_destroy(handle: *mut RifeHandleRaw);
}

// ---------------------------------------------------------------------------
// GPU lifecycle (process-lifetime singleton)
// ---------------------------------------------------------------------------

/// Initialise the Vulkan/ncnn GPU sub-system.
/// Must be called once before any `RifeInstance` is created.
pub fn init_gpu() -> bool {
    unsafe { rife_init_gpu() == 0 }
}

/// Tear down the Vulkan/ncnn GPU sub-system.
/// Must be called after all `RifeInstance` objects have been dropped.
pub fn cleanup_gpu() {
    unsafe { rife_cleanup_gpu() }
}

/// Return the number of Vulkan-capable GPU devices.
pub fn gpu_count() -> i32 {
    unsafe { rife_gpu_count() }
}

// ---------------------------------------------------------------------------
// Safe RIFE instance wrapper
// ---------------------------------------------------------------------------

/// A loaded RIFE model instance.  Creates and destroys the underlying C++ RIFE
/// object on construction / drop.
pub struct RifeInstance {
    ptr: *mut RifeHandleRaw,
}

// RIFE internally runs Vulkan commands on a dedicated GPU queue; the pointer
// is not shared across threads simultaneously during `process_v4`.
unsafe impl Send for RifeInstance {}

impl RifeInstance {
    /// Load a RIFE model from `model_dir` on GPU `gpuid`.
    ///
    /// Returns `None` if the model directory cannot be loaded or the GPU is
    /// unavailable.
    pub fn new(gpuid: i32, model_dir: &str) -> Option<Self> {
        let cpath = CString::new(model_dir).ok()?;
        let ptr = unsafe { rife_create(gpuid, cpath.as_ptr()) };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr })
        }
    }

    /// Interpolate one frame between `src0` and `src1` into `dst`.
    ///
    /// All slices are planar float32 (R plane, G plane, B plane) each of
    /// length `w * h`.  `timestep` is typically `0.5` for a midpoint frame.
    ///
    /// Returns `true` on success.
    pub fn process_v4(
        &self,
        src0: &PlanarFrame,
        src1: &PlanarFrame,
        dst:  &mut PlanarFrame,
        w: i32, h: i32,
        timestep: f32,
    ) -> bool {
        let stride = (w as isize) * std::mem::size_of::<f32>() as isize;
        let ret = unsafe {
            rife_process_v4(
                self.ptr,
                src0.r.as_ptr(), src0.g.as_ptr(), src0.b.as_ptr(),
                src1.r.as_ptr(), src1.g.as_ptr(), src1.b.as_ptr(),
                dst.r.as_mut_ptr(), dst.g.as_mut_ptr(), dst.b.as_mut_ptr(),
                w, h, stride,
                timestep,
            )
        };
        ret == 0
    }
}

impl Drop for RifeInstance {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { rife_destroy(self.ptr) };
            self.ptr = std::ptr::null_mut();
        }
    }
}

// ---------------------------------------------------------------------------
// Planar frame buffer
// ---------------------------------------------------------------------------

/// One video frame in planar float32 RGB (R plane, G plane, B plane).
#[derive(Clone)]
pub struct PlanarFrame {
    pub r: Vec<f32>,
    pub g: Vec<f32>,
    pub b: Vec<f32>,
}

impl PlanarFrame {
    pub fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            r: vec![0.0f32; n],
            g: vec![0.0f32; n],
            b: vec![0.0f32; n],
        }
    }
}

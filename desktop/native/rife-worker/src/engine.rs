//! Safe wrapper over the `rife_c` shim (cpp/rife_c.h) around the VS-RIFE ncnn
//! engine.

use std::ffi::{c_char, c_int, CStr, CString};

extern "C" {
    fn rife_init_gpu() -> c_int;
    fn rife_cleanup_gpu();
    fn rife_gpu_count() -> c_int;
    fn rife_default_gpu() -> c_int;
    fn rife_gpu_name(gpuid: c_int) -> *const c_char;
    fn rife_create(gpuid: c_int, model_dir: *const c_char) -> *mut ();
    fn rife_process_v4(
        handle: *mut (),
        src0_r: *const f32,
        src0_g: *const f32,
        src0_b: *const f32,
        src1_r: *const f32,
        src1_g: *const f32,
        src1_b: *const f32,
        dst_r: *mut f32,
        dst_g: *mut f32,
        dst_b: *mut f32,
        w: c_int,
        h: c_int,
        stride: isize,
        timestep: f32,
    ) -> c_int;
    fn rife_destroy(handle: *mut ());
}

/// Planar float32 RGB frame in 0..1, tightly packed.
pub struct PlanarFrame {
    pub r: Vec<f32>,
    pub g: Vec<f32>,
    pub b: Vec<f32>,
    pub width: usize,
    pub height: usize,
}

impl PlanarFrame {
    pub fn new(width: usize, height: usize) -> Self {
        let pixels = width * height;
        Self {
            r: vec![0.0; pixels],
            g: vec![0.0; pixels],
            b: vec![0.0; pixels],
            width,
            height,
        }
    }

    /// Fills the planes from interleaved rgb24 bytes.
    pub fn load_rgb24(&mut self, rgb: &[u8]) {
        const SCALE: f32 = 1.0 / 255.0;
        debug_assert_eq!(rgb.len(), self.r.len() * 3);
        for (((pixel, r), g), b) in rgb
            .chunks_exact(3)
            .zip(&mut self.r)
            .zip(&mut self.g)
            .zip(&mut self.b)
        {
            *r = pixel[0] as f32 * SCALE;
            *g = pixel[1] as f32 * SCALE;
            *b = pixel[2] as f32 * SCALE;
        }
    }

    /// Writes the planes as interleaved rgb24 bytes into `rgb`.
    pub fn store_rgb24(&self, rgb: &mut Vec<u8>) {
        rgb.resize(self.r.len() * 3, 0);
        let to_byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        for (((pixel, r), g), b) in rgb
            .chunks_exact_mut(3)
            .zip(&self.r)
            .zip(&self.g)
            .zip(&self.b)
        {
            pixel[0] = to_byte(*r);
            pixel[1] = to_byte(*g);
            pixel[2] = to_byte(*b);
        }
    }
}

/// The Vulkan/ncnn GPU subsystem. Create once per process; RIFE instances
/// must be dropped before it.
pub struct Gpu(());

impl Gpu {
    pub fn init() -> Result<Self, String> {
        if unsafe { rife_init_gpu() } != 0 {
            return Err("failed to initialize Vulkan (no Vulkan driver?)".to_string());
        }
        if unsafe { rife_gpu_count() } <= 0 {
            unsafe { rife_cleanup_gpu() };
            return Err("no Vulkan-capable GPU found".to_string());
        }
        Ok(Self(()))
    }

    /// The GPU ncnn prefers, a discrete GPU when present.
    pub fn default_index(&self) -> i32 {
        unsafe { rife_default_gpu() }
    }

    pub fn name(&self, index: i32) -> Option<String> {
        let name = unsafe { rife_gpu_name(index) };
        (!name.is_null()).then(|| {
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        })
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe { rife_cleanup_gpu() };
    }
}

/// A loaded RIFE model on one GPU.
pub struct Rife<'gpu> {
    handle: *mut (),
    _gpu: std::marker::PhantomData<&'gpu Gpu>,
}

impl<'gpu> Rife<'gpu> {
    pub fn load(_gpu: &'gpu Gpu, gpu_index: i32, model_dir: &str) -> Result<Self, String> {
        let path = CString::new(model_dir).map_err(|_| "model path contains NUL".to_string())?;
        let handle = unsafe { rife_create(gpu_index, path.as_ptr()) };
        if handle.is_null() {
            return Err(format!("failed to load RIFE model from '{model_dir}'"));
        }
        Ok(Self {
            handle,
            _gpu: std::marker::PhantomData,
        })
    }

    /// Writes the frame at `timestep` (0..1) between `from` and `to` into
    /// `out`. All three frames must share dimensions.
    pub fn interpolate(
        &self,
        from: &PlanarFrame,
        to: &PlanarFrame,
        timestep: f32,
        out: &mut PlanarFrame,
    ) -> Result<(), String> {
        assert!(from.width == to.width && from.height == to.height);
        assert!(out.width == from.width && out.height == from.height);
        let status = unsafe {
            rife_process_v4(
                self.handle,
                from.r.as_ptr(),
                from.g.as_ptr(),
                from.b.as_ptr(),
                to.r.as_ptr(),
                to.g.as_ptr(),
                to.b.as_ptr(),
                out.r.as_mut_ptr(),
                out.g.as_mut_ptr(),
                out.b.as_mut_ptr(),
                out.width as c_int,
                out.height as c_int,
                // Elements, not bytes: the planes are tightly packed.
                out.width as isize,
                timestep,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("RIFE inference failed ({status})"))
        }
    }
}

// SAFETY: `RIFE::process_v4` is const and takes its Vulkan allocators and
// command buffers per call, so one instance serves concurrent callers; the
// VapourSynth plugin calls it from several frame threads the same way.
unsafe impl Send for Rife<'_> {}
unsafe impl Sync for Rife<'_> {}

impl Drop for Rife<'_> {
    fn drop(&mut self) {
        unsafe { rife_destroy(self.handle) };
    }
}

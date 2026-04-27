fn main() {
    // Build the VS-RIFE C++ engine and the rife_c shim only when the
    // `rife-ffi` feature is enabled (i.e. when building the rife-worker
    // binary target in a release or development build that has GPU/Vulkan
    // support available).
    if std::env::var("CARGO_FEATURE_RIFE_FFI").is_ok() {
        build_rife_ffi();
    }

    tauri_build::build()
}

/// Build ncnn (CMake) then compile the RIFE C++ sources and our rife_c shim
/// into a static library that rife_worker can link against.
fn build_rife_ffi() {
    use std::path::PathBuf;

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    // -----------------------------------------------------------------------
    // 1. Build ncnn via CMake
    // -----------------------------------------------------------------------
    let ncnn_src = manifest_dir.join("vendor").join("ncnn");
    let ncnn_dst = cmake::Config::new(&ncnn_src)
        // Core Vulkan configuration.
        .define("NCNN_VULKAN", "ON")
        .define("NCNN_VULKAN_ONLINE_SPIRV", "ON")
        .define("NCNN_INSTALL_SDK", "ON")
        // Disable unused build targets.
        .define("NCNN_BUILD_BENCHMARK", "OFF")
        .define("NCNN_BUILD_TESTS", "OFF")
        .define("NCNN_BUILD_TOOLS", "OFF")
        .define("NCNN_BUILD_EXAMPLES", "OFF")
        .define("NCNN_INT8", "OFF")
        .define("NCNN_AVX512", "OFF")
        .define("NCNN_DISABLE_RTTI", "OFF")
        .define("NCNN_DISABLE_EXCEPTION", "OFF")
        // Disable unused layers to minimise binary size (mirrors VS-RIFE meson.build).
        .define("WITH_LAYER_concat", "ON")
        .define("WITH_LAYER_convolution", "ON")
        .define("WITH_LAYER_crop", "ON")
        .define("WITH_LAYER_deconvolution", "ON")
        .define("WITH_LAYER_eltwise", "ON")
        .define("WITH_LAYER_flatten", "ON")
        .define("WITH_LAYER_innerproduct", "ON")
        .define("WITH_LAYER_input", "ON")
        .define("WITH_LAYER_memorydata", "ON")
        .define("WITH_LAYER_pooling", "ON")
        .define("WITH_LAYER_prelu", "ON")
        .define("WITH_LAYER_relu", "ON")
        .define("WITH_LAYER_sigmoid", "ON")
        .define("WITH_LAYER_slice", "ON")
        .define("WITH_LAYER_split", "ON")
        .define("WITH_LAYER_binaryop", "ON")
        .define("WITH_LAYER_unaryop", "ON")
        .define("WITH_LAYER_convolutiondepthwise", "ON")
        .define("WITH_LAYER_padding", "ON")
        .define("WITH_LAYER_interp", "ON")
        .define("WITH_LAYER_clip", "ON")
        .define("WITH_LAYER_packing", "ON")
        .define("WITH_LAYER_cast", "ON")
        .define("WITH_LAYER_pixelshuffle", "ON")
        .define("WITH_LAYER_gemm", "ON")
        .profile("Release")
        .build();

    let lib_dir = ncnn_dst.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ncnn");

    // ncnn bundles glslang for online SPIR-V compilation.
    // Link all glslang sub-libraries that ncnn depends on.
    for lib in &[
        "glslang",
        "SPIRV",
        "MachineIndependent",
        "GenericCodeGen",
        "OSDependent",
    ] {
        // The library may or may not exist depending on the ncnn/glslang
        // version; emit the directive either way — the linker will ignore
        // missing static archives that are not actually referenced.
        let candidate = lib_dir.join(if cfg!(windows) {
            format!("{lib}.lib")
        } else {
            format!("lib{lib}.a")
        });
        if candidate.exists() {
            println!("cargo:rustc-link-lib=static={lib}");
        }
    }

    // Vulkan loader (dynamic — must be present at runtime on the target machine).
    if cfg!(target_os = "windows") {
        // The Vulkan SDK installer sets VULKAN_SDK; fall back to a sensible
        // default so the link works even without the env var (the SDK headers
        // are still needed, but CMake above handles finding them).
        if let Ok(sdk) = std::env::var("VULKAN_SDK") {
            println!("cargo:rustc-link-search=native={sdk}\\Lib");
        }
        println!("cargo:rustc-link-lib=dylib=vulkan-1");
    } else {
        println!("cargo:rustc-link-lib=dylib=vulkan");
    }

    // Platform-specific system libraries required by ncnn.
    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
    }

    // -----------------------------------------------------------------------
    // 2. Compile RIFE C++ sources and the rife_c shim with the cc crate
    // -----------------------------------------------------------------------
    let ncnn_include = ncnn_dst.join("include");
    let ncnn_include_sub = ncnn_dst.join("include").join("ncnn");
    let rife_src_dir = manifest_dir.join("vendor").join("vs-rife").join("RIFE");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .opt_level(3)
        // ncnn headers.
        .include(&ncnn_include)
        .include(&ncnn_include_sub)
        // RIFE headers.
        .include(&rife_src_dir)
        // Source files.
        .file(rife_src_dir.join("rife.cpp"))
        .file(rife_src_dir.join("warp.cpp"))
        .file(manifest_dir.join("cpp").join("rife_c.cpp"))
        // Suppress unused-parameter warnings from vendored code.
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-missing-field-initializers")
        .compile("rife_shim");

    // Re-run the build script if any of these change.
    println!("cargo:rerun-if-changed=cpp/rife_c.h");
    println!("cargo:rerun-if-changed=cpp/rife_c.cpp");
    println!("cargo:rerun-if-changed=vendor/vs-rife/RIFE/rife.h");
    println!("cargo:rerun-if-changed=vendor/vs-rife/RIFE/rife.cpp");
    println!("cargo:rerun-if-changed=vendor/vs-rife/RIFE/warp.cpp");
}

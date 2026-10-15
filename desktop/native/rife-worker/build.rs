//! Builds ncnn (CMake) and the VS-RIFE engine plus the `rife_c` shim (cc) into
//! static libraries for the `engine` feature. The sources come from
//! `scripts/fetch-rife-vendor.js`, which pins ncnn and VS-RIFE to the versions
//! that match the RIFE models the app downloads.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=cpp/rife_c.h");
    println!("cargo:rerun-if-changed=cpp/rife_c.cpp");
    println!("cargo:rerun-if-changed=vendor/.rife-vendor.json");

    if std::env::var("CARGO_FEATURE_ENGINE").is_err() {
        return;
    }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("vendor");
    if !vendor_dir.join(".rife-vendor.json").is_file() {
        panic!(
            "rife-worker vendor sources are missing from {}. Run `npm run fetch:rife` \
             (scripts/fetch-rife-vendor.js) first.",
            vendor_dir.display()
        );
    }

    let ncnn_dst = build_ncnn(&vendor_dir.join("ncnn"));
    build_engine(
        &manifest_dir,
        &vendor_dir.join("vs-rife").join("RIFE"),
        &ncnn_dst,
    );
}

/// Builds ncnn with Vulkan and online SPIR-V compilation (RIFE compiles its
/// GLSL shaders at load time) and links it and its glslang libraries.
fn build_ncnn(ncnn_src: &Path) -> PathBuf {
    let mut config = cmake::Config::new(ncnn_src);
    if static_msvc_crt() {
        // Match Rust's static CRT (.cargo/config.toml); CMake otherwise
        // compiles with /MD and the link fails on __imp_ CRT symbols.
        config
            .static_crt(true)
            .define("CMAKE_POLICY_DEFAULT_CMP0091", "NEW")
            .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded");
    }
    let ncnn_dst = config
        .define("NCNN_VULKAN", "ON")
        .define("NCNN_VULKAN_ONLINE_SPIRV", "ON")
        // ncnn's in-house loader opens the system Vulkan library at runtime,
        // so no Vulkan SDK is needed to build and no vulkan-1.lib is linked.
        .define("NCNN_SIMPLEVK", "ON")
        .define("NCNN_SHARED_LIB", "OFF")
        .define("NCNN_INSTALL_SDK", "ON")
        // RIFE does its CPU work single-threaded; skipping OpenMP avoids a
        // runtime dependency on the OpenMP DLL (vcomp140.dll on Windows).
        .define("NCNN_OPENMP", "OFF")
        .define("NCNN_BUILD_BENCHMARK", "OFF")
        .define("NCNN_BUILD_TESTS", "OFF")
        .define("NCNN_BUILD_TOOLS", "OFF")
        .define("NCNN_BUILD_EXAMPLES", "OFF")
        .define("NCNN_PIXEL_ROTATE", "OFF")
        .define("NCNN_PIXEL_AFFINE", "OFF")
        .define("NCNN_PIXEL_DRAWING", "OFF")
        .define("NCNN_INT8", "OFF")
        .define("NCNN_AVX512", "OFF")
        .profile("Release")
        .build();

    let lib_dir = ncnn_dst.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ncnn");
    // glslang's library set varies between versions; link whatever ncnn
    // installed.
    for lib in [
        "glslang",
        "SPIRV",
        "MachineIndependent",
        "GenericCodeGen",
        "OSDependent",
        "OGLCompiler",
        "glslang-default-resource-limits",
    ] {
        let file = if cfg!(windows) {
            format!("{lib}.lib")
        } else {
            format!("lib{lib}.a")
        };
        if lib_dir.join(file).exists() {
            println!("cargo:rustc-link-lib=static={lib}");
        }
    }

    if cfg!(target_os = "linux") {
        // simplevk loads libvulkan with dlopen.
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=pthread");
    }

    ncnn_dst
}

fn static_msvc_crt() -> bool {
    std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
        && std::env::var("CARGO_CFG_TARGET_FEATURE")
            .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"))
}

/// Compiles the VS-RIFE engine and the `rife_c` C-linkage shim.
fn build_engine(manifest_dir: &Path, rife_src: &Path, ncnn_dst: &Path) {
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .opt_level(3)
        .include(ncnn_dst.join("include"))
        .include(ncnn_dst.join("include").join("ncnn"))
        .include(rife_src)
        .define("NOMINMAX", None)
        .file(rife_src.join("rife.cpp"))
        .file(rife_src.join("warp.cpp"))
        .file(manifest_dir.join("cpp").join("rife_c.cpp"))
        .flag_if_supported("/utf-8")
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-missing-field-initializers")
        .compile("rife_shim");
}

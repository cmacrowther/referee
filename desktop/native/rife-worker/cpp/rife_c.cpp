// C-linkage shim over the VS-RIFE C++ RIFE class.
// See rife_c.h for the public API.

#include "rife_c.h"

#include "rife.h"  // vendor/vs-rife/RIFE

// ncnn GPU lifecycle.
#include "gpu.h"
#include "net.h"

#include <string>

#if _WIN32
#include <windows.h>
#endif

// ---------------------------------------------------------------------------
// Internal helpers: derive RIFE constructor parameters from model path
// ---------------------------------------------------------------------------

static bool detect_rife_v4(const std::string& path)
{
    // Matches VapourSynth plugin logic.
    if (path.find("rife-v2")   != std::string::npos) return false;
    if (path.find("rife-v3.9") != std::string::npos) return true;   // 3.9 uses v4 flow
    if (path.find("rife-v3")   != std::string::npos) return false;
    if (path.find("rife-v4")   != std::string::npos) return true;
    if (path.find("rife4")     != std::string::npos) return true;
    return false;
}

static bool detect_rife_v2(const std::string& path)
{
    if (path.find("rife-v2")   != std::string::npos) return true;
    if (path.find("rife-v3.9") != std::string::npos) return false;
    if (path.find("rife-v3")   != std::string::npos) return true;
    return false;
}

static int detect_padding(const std::string& path)
{
    // rife-v4.25-lite must be checked before rife-v4.25.
    if (path.find("rife-v4.25-lite") != std::string::npos) return 128;
    if (path.find("rife-v4.25")      != std::string::npos) return 64;
    if (path.find("rife-v4.26")      != std::string::npos) return 64;
    return 32;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

extern "C" {

int rife_init_gpu(void)
{
    return ncnn::create_gpu_instance() == 0 ? 0 : 1;
}

void rife_cleanup_gpu(void)
{
    ncnn::destroy_gpu_instance();
}

int rife_gpu_count(void)
{
    return ncnn::get_gpu_count();
}

int rife_default_gpu(void)
{
    return ncnn::get_default_gpu_index();
}

const char* rife_gpu_name(int gpuid)
{
    if (gpuid < 0 || gpuid >= ncnn::get_gpu_count()) return nullptr;
    return ncnn::get_gpu_info(gpuid).device_name();
}

RifeHandle rife_create(int gpuid, const char* model_dir)
{
    if (!model_dir) return nullptr;
    if (gpuid < 0) gpuid = ncnn::get_default_gpu_index();

    std::string modelPath(model_dir);
    bool rv4     = detect_rife_v4(modelPath);
    bool rv2     = detect_rife_v2(modelPath);
    int  padding = detect_padding(modelPath);

    RIFE* rife = nullptr;
    try {
        rife = new RIFE(gpuid, /*tta_mode=*/false, /*uhd_mode=*/false,
                        /*num_threads=*/1, rv2, rv4, padding);
#if _WIN32
        // RIFE::load takes a wide path on Windows.
        int wlen = MultiByteToWideChar(CP_UTF8, 0, model_dir, -1, nullptr, 0);
        if (wlen <= 0) { delete rife; return nullptr; }
        std::wstring wmodelPath(static_cast<size_t>(wlen - 1), L'\0');
        MultiByteToWideChar(CP_UTF8, 0, model_dir, -1, wmodelPath.data(), wlen);
        int ret = rife->load(wmodelPath);
#else
        int ret = rife->load(modelPath);
#endif
        if (ret != 0) { delete rife; return nullptr; }
        return static_cast<RifeHandle>(rife);
    } catch (...) {
        delete rife;
        return nullptr;
    }
}

int rife_process_v4(
    RifeHandle   handle,
    const float* src0R, const float* src0G, const float* src0B,
    const float* src1R, const float* src1G, const float* src1B,
    float*       dstR,  float*       dstG,  float*       dstB,
    int w, int h, ptrdiff_t stride,
    float timestep)
{
    if (!handle) return -1;
    const RIFE* rife = static_cast<const RIFE*>(handle);
    return rife->process_v4(
        src0R, src0G, src0B,
        src1R, src1G, src1B,
        dstR,  dstG,  dstB,
        w, h, stride, timestep);
}

void rife_destroy(RifeHandle handle)
{
    if (handle) {
        delete static_cast<RIFE*>(handle);
    }
}

} // extern "C"

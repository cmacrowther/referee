#!/usr/bin/env node
// Fetches the pinned native sources the rife-worker binary builds against (see
// desktop/native/rife-worker/build.rs) into desktop/native/rife-worker/vendor/.
// Re-running is a no-op once the pinned versions are present.
const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const VENDOR_DIR = path.resolve(__dirname, "../desktop/native/rife-worker/vendor");
const STAMP_PATH = path.join(VENDOR_DIR, ".rife-vendor.json");

// The ncnn release VS-RIFE r9_mod_v33 pins (subprojects/ncnn @ 305837f).
const NCNN = { repo: "https://github.com/Tencent/ncnn.git", tag: "20250503" };

// styler00dollar/VapourSynth-RIFE-ncnn-Vulkan r9_mod_v33, the tag whose models
// the app downloads. Only the RIFE engine sources are fetched: the repository
// itself is over 1 GB of models.
const VS_RIFE = {
    repo: "styler00dollar/VapourSynth-RIFE-ncnn-Vulkan",
    commit: "c3ec6aabc07c8fa37a4f58d7fed9e2ad1fc1b13f",
    files: [
        "LICENSE",
        "RIFE/rife.cpp",
        "RIFE/rife.h",
        "RIFE/rife_ops.h",
        "RIFE/warp.cpp",
        "RIFE/rife_flow_tta_avg.comp.hex.h",
        "RIFE/rife_flow_tta_temporal_avg.comp.hex.h",
        "RIFE/rife_out_tta_temporal_avg.comp.hex.h",
        "RIFE/rife_postproc.comp.hex.h",
        "RIFE/rife_postproc_tta.comp.hex.h",
        "RIFE/rife_preproc.comp.hex.h",
        "RIFE/rife_preproc_tta.comp.hex.h",
        "RIFE/rife_v2_flow_tta_avg.comp.hex.h",
        "RIFE/rife_v2_flow_tta_temporal_avg.comp.hex.h",
        "RIFE/rife_v4_timestep.comp.hex.h",
        "RIFE/warp.comp.hex.h",
        "RIFE/warp_pack4.comp.hex.h",
        "RIFE/warp_pack8.comp.hex.h",
    ],
};

const PINNED = { ncnn: NCNN.tag, vsRife: VS_RIFE.commit };

function readStamp() {
    try {
        return JSON.parse(fs.readFileSync(STAMP_PATH, "utf8"));
    } catch {
        return null;
    }
}

function git(args, cwd) {
    execFileSync("git", args, { cwd, stdio: "inherit" });
}

function fetchNcnn() {
    const dest = path.join(VENDOR_DIR, "ncnn");
    fs.rmSync(dest, { recursive: true, force: true });
    console.log(`Cloning ncnn ${NCNN.tag}...`);
    git(["clone", "--depth", "1", "--branch", NCNN.tag, "--no-tags", NCNN.repo, dest]);
    // glslang compiles RIFE's GLSL shaders at runtime (NCNN_VULKAN_ONLINE_SPIRV).
    git(["submodule", "update", "--init", "--depth", "1", "glslang"], dest);
}

async function fetchVsRife() {
    const dest = path.join(VENDOR_DIR, "vs-rife");
    fs.rmSync(dest, { recursive: true, force: true });
    console.log(`Fetching VS-RIFE engine sources at ${VS_RIFE.commit.slice(0, 7)}...`);
    for (const file of VS_RIFE.files) {
        const url = `https://raw.githubusercontent.com/${VS_RIFE.repo}/${VS_RIFE.commit}/${file}`;
        const response = await fetch(url);
        if (!response.ok) {
            throw new Error(`GET ${url} failed: ${response.status} ${response.statusText}`);
        }
        const target = path.join(dest, file);
        fs.mkdirSync(path.dirname(target), { recursive: true });
        fs.writeFileSync(target, Buffer.from(await response.arrayBuffer()));
    }
}

async function main() {
    const stamp = readStamp();
    if (stamp && stamp.ncnn === PINNED.ncnn && stamp.vsRife === PINNED.vsRife) {
        console.log("rife-worker vendor sources are up to date.");
        return;
    }

    fs.mkdirSync(VENDOR_DIR, { recursive: true });
    if (stamp?.ncnn !== PINNED.ncnn) {
        fetchNcnn();
    }
    if (stamp?.vsRife !== PINNED.vsRife) {
        await fetchVsRife();
    }
    fs.writeFileSync(STAMP_PATH, JSON.stringify(PINNED, null, 4) + "\n");
    console.log(`rife-worker vendor sources ready in ${VENDOR_DIR}`);
}

main().catch(error => {
    console.error(error.message);
    process.exit(1);
});

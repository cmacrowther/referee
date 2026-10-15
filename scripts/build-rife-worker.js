#!/usr/bin/env node
// Builds rife-worker (desktop/native/rife-worker) and stages it where Tauri
// bundles it as a sidecar next to the app executable
// (desktop/native/binaries/rife-worker-<target-triple>[.exe], see
// desktop/native/tauri.rife.conf.json). Needs a C++ toolchain and CMake; no
// Vulkan SDK.
const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const WORKER_DIR = path.join(ROOT, "desktop/native/rife-worker");
const BINARIES_DIR = path.join(ROOT, "desktop/native/binaries");
const EXE = process.platform === "win32" ? ".exe" : "";

function hostTriple() {
    const info = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
    const host = info.split("\n").find(line => line.startsWith("host:"));
    if (!host) {
        throw new Error("could not read the Rust host target from `rustc -vV`");
    }
    return host.slice("host:".length).trim();
}

function onPath(program) {
    try {
        execFileSync(program, ["--version"], { stdio: "ignore" });
        return true;
    } catch {
        return false;
    }
}

/// CMake for the ncnn build. On Windows, falls back to the copy Visual Studio
/// (or its Build Tools) installs when `cmake` is not on PATH.
function cmakeEnv() {
    if (process.env.CMAKE || onPath("cmake") || process.platform !== "win32") {
        return {};
    }
    const vswhere = path.join(
        process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)",
        "Microsoft Visual Studio/Installer/vswhere.exe"
    );
    if (!fs.existsSync(vswhere)) {
        return {};
    }
    const installation = execFileSync(
        vswhere,
        ["-products", "*", "-latest", "-property", "installationPath"],
        { encoding: "utf8" }
    ).trim();
    const cmake = path.join(
        installation,
        "Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe"
    );
    return fs.existsSync(cmake) ? { CMAKE: cmake } : {};
}

function main() {
    execFileSync(process.execPath, [path.join(__dirname, "fetch-rife-vendor.js")], {
        stdio: "inherit",
    });

    console.log("Building rife-worker...");
    // Run from the crate directory so its .cargo/config.toml (static CRT on
    // Windows) applies.
    execFileSync("cargo", ["build", "--release", "--locked"], {
        cwd: WORKER_DIR,
        stdio: "inherit",
        env: { ...process.env, ...cmakeEnv() },
    });

    const built = path.join(WORKER_DIR, "target/release", `rife-worker${EXE}`);
    const staged = path.join(BINARIES_DIR, `rife-worker-${hostTriple()}${EXE}`);
    fs.mkdirSync(BINARIES_DIR, { recursive: true });
    fs.copyFileSync(built, staged);
    console.log(`rife-worker staged at ${staged}`);
}

try {
    main();
} catch (error) {
    console.error(error.message);
    process.exit(1);
}

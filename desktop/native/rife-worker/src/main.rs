//! rife-worker: the REFEREE pipeline's RIFE preprocess stage. Reads NUT
//! (rawvideo + audio) on stdin, converts the frame rate with RIFE on the GPU
//! (ncnn/Vulkan, any vendor), and writes NUT on stdout with the audio passed
//! through.
//!
//! Spawned by the app's `StreamingRifePreprocessor`. Prints
//! `[rife_worker] ready` to stderr once the model is loaded, and exits when
//! stdin ends. `--probe` checks that RIFE runs on this machine.

#![cfg_attr(not(feature = "engine"), allow(dead_code))]

mod args;
mod ffmpeg;
mod nut;
mod timeline;

#[cfg(feature = "engine")]
mod engine;
#[cfg(feature = "engine")]
mod worker;

fn main() {
    #[cfg(feature = "engine")]
    std::process::exit(worker::main());

    #[cfg(not(feature = "engine"))]
    {
        eprintln!("[rife_worker] fatal: built without the RIFE engine");
        std::process::exit(1);
    }
}

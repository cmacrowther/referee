#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Program entry point that runs the referee runtime.
fn main() {
    referee_lib::run()
}

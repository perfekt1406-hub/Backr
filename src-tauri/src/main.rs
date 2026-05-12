/*
 * Native entrypoint for the Backr Tauri binary; delegates to `backr_lib::run()`.
 */

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    backr_lib::run();
}

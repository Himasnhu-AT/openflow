// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = jezflow_lib::cli::dispatch(&args) {
        std::process::exit(code);
    }
    jezflow_lib::run()
}

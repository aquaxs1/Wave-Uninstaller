// Without this the app opens a console window behind the UI in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wave_app_lib::run()
}

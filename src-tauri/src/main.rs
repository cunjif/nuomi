//! Windows entrypoint for the nuomi desktop shell.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nuomi_shell_lib::run();
}

//! Standalone TS-bindings generator (`pnpm contracts:gen`).

fn main() {
    if let Err(e) = nuomi_shell_lib::export_bindings() {
        eprintln!("contracts:gen failed: {e}");
        std::process::exit(1);
    }
    println!("bindings written to src/lib/ipc/bindings.gen.ts");
}

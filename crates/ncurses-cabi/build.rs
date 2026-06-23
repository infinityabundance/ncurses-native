// Make the cdylib a real drop-in: set the ncurses soname and version the exported
// symbols under the same version nodes the system libraries use, so a dynamic
// linker resolves them the same way (gap-ledger STRUCT-02/ABI-VERS-01).
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libtinfo.so.6");
    println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script={dir}/version.map");
    println!("cargo:rerun-if-changed=version.map");
    println!("cargo:rerun-if-changed=build.rs");
}

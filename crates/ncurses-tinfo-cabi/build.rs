// The standalone libtinfo split: soname libtinfo.so.6, symbols versioned under the tinfo node and
// restricted to the tinfo subset by version.map (gap-ledger BLD-05).
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libtinfo.so.6");
    println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script={dir}/version.map");
    println!("cargo:rerun-if-changed=version.map");
    println!("cargo:rerun-if-changed=build.rs");
}

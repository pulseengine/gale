// Link the dissolved gust_mix (synth -b riscv --target esp32c3, RV32IMC).
// Checked in next to this crate so the board build needs no dissolve toolchain.
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg={}/gust_mix-esp32c3.o", manifest);
    println!("cargo:rerun-if-changed=gust_mix-esp32c3.o");
}

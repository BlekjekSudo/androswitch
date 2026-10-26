fn main() {
    // These branches belong to the other build of this same source tree.
    // Recognize their cfg names without exposing Cargo switches that could
    // enable desktop state in the device package through feature unification.
    println!("cargo:rustc-check-cfg=cfg(feature, values(\"std\", \"native\", \"jit\", \"advanced-components\", \"virtual-devices\"))");
}

fn main() {
    slint_build::compile("ui/wifi.slint").unwrap();
    println!("cargo:rerun-if-changed=ui/wifi.slint");
}

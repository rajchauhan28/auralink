fn main() {
    slint_build::compile("ui/wifi.slint").unwrap();
    slint_build::compile("ui/bluetooth.slint").unwrap();
    println!("cargo:rerun-if-changed=ui/wifi.slint");
    println!("cargo:rerun-if-changed=ui/bluetooth.slint");
}

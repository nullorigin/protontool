fn main() {
    println!("cargo::rerun-if-env-changed=protontool_DEFAULT_STEAM_DIR");
    println!("cargo::rerun-if-env-changed=protontool_DEFAULT_GUI_PROVIDER");
    println!("cargo::rerun-if-env-changed=protontool_STEAM_RUNTIME_PATH");
}

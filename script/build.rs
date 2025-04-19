use std::env;
use std::fs;
use std::path::Path;

fn main() {
    if env::var("CARGO_FEATURE_NATIVE").is_ok() {
        // When the native feature is enabled, use the normal build system.
        println!("cargo:warning=Using native build system instead of sp1_helper::build_program");
        let program_path = "../program/utreexo";
        add_rerun_if_changed_for_dir(Path::new(program_path));
        // Optionally, you could add further native-specific build commands here.
    } else {
        // Call the custom build.
        sp1_build::build_program("../program/utreexo");
    }
}

// Recursively add each file/directory under `dir` for change detection.
fn add_rerun_if_changed_for_dir(dir: &Path) {
    if dir.is_dir() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            add_rerun_if_changed_for_dir(&path);
        }
    } else {
        println!("cargo:rerun-if-changed={}", dir.display());
    }
}

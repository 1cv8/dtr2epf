use crate::model::Project;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use walkdir::WalkDir;

pub fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("datareon_sample")
}

pub fn load_fixture() -> Project {
    let path = fixture_path();
    Project::load(&path)
        .unwrap_or_else(|error| panic!("failed to load test fixture {}: {error}", path.display()))
}

pub fn copy_fixture() -> TempDir {
    let source = fixture_path();
    let target = TempDir::new().expect("failed to create fixture copy directory");

    for entry in WalkDir::new(&source) {
        let entry = entry.expect("failed to enumerate test fixture");
        let relative = entry
            .path()
            .strip_prefix(&source)
            .expect("fixture entry is outside fixture root");
        let destination = target.path().join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination).expect("failed to create fixture directory");
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &destination).expect("failed to copy fixture file");
        }
    }

    target
}

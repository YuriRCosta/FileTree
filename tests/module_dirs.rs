use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn fixture() -> Vec<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join("tests/fixtures/module_dir_names.json")).unwrap();
    serde_json::from_str(&text).expect("fixture is JSON")
}

fn wrapper_rows() -> Vec<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(root.join("tests/fixtures/module_dir_names.js")).unwrap();
    assert!(source.starts_with(".pragma library\n"));
    let start = source.find("var NAMES = ").expect("wrapper header") + "var NAMES = ".len();
    let json = source[start..].trim().trim_end_matches(';');
    serde_json::from_str(json).expect("wrapper is JSON after the header")
}

struct Homes {
    _guard: MutexGuard<'static, ()>,
    root: PathBuf,
}

impl Homes {
    fn new(name: &str) -> Self {
        let guard = ENVIRONMENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!(
            "filetree-module-dirs-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", root.join("state"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        }
        Self {
            _guard: guard,
            root,
        }
    }

    fn state(&self, name: &str) -> PathBuf {
        self.root.join("state/omarchy/filetree/modules").join(name)
    }

    fn config(&self, name: &str) -> PathBuf {
        self.root.join("config/omarchy/filetree/config").join(name)
    }
}

impl Drop for Homes {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn dir_name_matches_the_shared_fixture_in_both_files() {
    let rows = fixture();
    assert_eq!(
        rows,
        wrapper_rows(),
        "the .js wrapper carries the same rows"
    );
    assert!(rows.len() >= 12);
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let expected = row["name"].as_str().unwrap();
        let actual = fileblade::module_dirs::dir_name(id).unwrap_or_default();
        assert_eq!(actual, expected, "dir_name({id:?})");
        if expected.is_empty() {
            assert!(fileblade::module_dirs::state_dir(id).is_none());
            assert!(fileblade::module_dirs::config_dir(id).is_none());
            assert!(fileblade::module_dirs::ensure(id).is_err());
        }
    }
}

#[test]
fn ensure_creates_both_private_directories_and_is_idempotent() {
    let homes = Homes::new("ensure");
    let first = fileblade::module_dirs::ensure("data-goblin.blade-example/clock").unwrap();
    let name = "data-goblin.blade-example+clock";
    assert_eq!(first["ok"], true);
    assert_eq!(first["module"], "data-goblin.blade-example/clock");
    assert_eq!(first["name"], name);
    assert_eq!(
        first["state_dir"],
        homes.state(name).to_string_lossy().as_ref()
    );
    assert_eq!(
        first["config_dir"],
        homes.config(name).to_string_lossy().as_ref()
    );
    assert!(homes.state(name).is_dir());
    assert!(homes.config(name).is_dir());
    assert_eq!(mode(&homes.state(name)), 0o700);
    assert_eq!(mode(&homes.config(name)), 0o700);
    assert!(
        !homes.root.join("config/omarchy/filetree/modules").exists(),
        "config never lands in the user-module scan root"
    );
    let second = fileblade::module_dirs::ensure("data-goblin.blade-example/clock").unwrap();
    assert_eq!(first, second);
    assert_eq!(
        fileblade::module_dirs::state_dir("notes").unwrap(),
        homes.state("notes")
    );
    assert_eq!(
        fileblade::module_dirs::config_dir("notes").unwrap(),
        homes.config("notes")
    );
}

#[test]
fn ensure_tightens_a_loose_directory_and_refuses_a_symlink_leaf() {
    let homes = Homes::new("modes");
    let loose = homes.state("notes");
    fs::create_dir_all(&loose).unwrap();
    fs::set_permissions(&loose, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(mode(&loose), 0o755);
    let document = fileblade::module_dirs::ensure("notes").unwrap();
    assert_eq!(document["ok"], true);
    assert_eq!(mode(&loose), 0o700);

    let elsewhere = homes.root.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(homes.config("files").parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, homes.config("files")).unwrap();
    let error = fileblade::module_dirs::ensure("files").unwrap_err();
    assert!(!error.to_string().is_empty());
    assert!(
        homes
            .config("files")
            .symlink_metadata()
            .unwrap()
            .is_symlink()
    );
    assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
}

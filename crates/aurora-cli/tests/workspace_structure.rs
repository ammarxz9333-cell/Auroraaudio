use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("aurora-cli must remain under crates/")
        .to_path_buf()
}

#[test]
fn preserved_experimental_crates_remain_present_but_excluded() {
    let root = repository_root();
    let workspace = fs::read_to_string(root.join("Cargo.toml")).expect("read workspace manifest");

    for path in [
        "crates/aurora-renderer-cavern",
        "crates/aurora-decoder-truehdd",
    ] {
        assert!(root.join(path).join("Cargo.toml").is_file(), "missing {path}/Cargo.toml");
        assert!(root.join(path).join("src/lib.rs").is_file(), "missing {path}/src/lib.rs");
        assert!(
            workspace.contains(&format!("\"{path}\"")),
            "workspace manifest must record preserved path {path}"
        );
    }

    let members = workspace
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("workspace members array");
    assert!(!members.contains("aurora-renderer-cavern"));
    assert!(!members.contains("aurora-decoder-truehdd"));

    let excluded = workspace
        .split("exclude = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("workspace exclude array");
    assert!(excluded.contains("aurora-renderer-cavern"));
    assert!(excluded.contains("aurora-decoder-truehdd"));
}

#[test]
fn active_manifests_do_not_depend_on_preserved_experiments() {
    let root = repository_root();
    let crates = root.join("crates");
    let preserved = ["aurora-renderer-cavern", "aurora-decoder-truehdd"];

    for entry in fs::read_dir(crates).expect("read crates directory") {
        let entry = entry.expect("read crate entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if preserved.contains(&name.as_ref()) {
            continue;
        }
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = fs::read_to_string(&manifest).expect("read crate manifest");
        for preserved_name in preserved {
            assert!(
                !text.contains(preserved_name),
                "active manifest {} depends on preserved experimental crate {preserved_name}",
                manifest.display()
            );
        }
    }
}

use regex::Regex;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn markdown_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
}

#[test]
fn documentation_links_resolve_and_root_stays_focused() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root_markdown = fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().is_some_and(|extension| extension == "md")).then_some(path)
        })
        .collect::<Vec<_>>();
    assert_eq!(root_markdown, [root.join("README.md")]);

    let mut files = vec![root.join("README.md")];
    markdown_files(&root.join("docs"), &mut files);
    let links = Regex::new(r#"\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)"#).unwrap();
    let mut failures = Vec::new();
    for file in files {
        let contents = fs::read_to_string(&file).unwrap();
        for captures in links.captures_iter(&contents) {
            let raw = captures[1].trim_matches(['<', '>']);
            if raw.starts_with('#') || raw.starts_with("mailto:") || raw.contains("://") {
                continue;
            }
            let relative = raw.split('#').next().unwrap();
            let target = file.parent().unwrap().join(relative);
            if !target.exists() {
                failures.push(format!(
                    "{} links to missing {}",
                    file.strip_prefix(root).unwrap().display(),
                    raw
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn beta_entry_points_match_the_package_and_default_config() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains(env!("CARGO_PKG_VERSION")));

    let config: Value =
        serde_json::from_str(&fs::read_to_string(root.join("positorium.json")).unwrap()).unwrap();
    assert_eq!(config["listen_interface"], "127.0.0.1");
    assert_eq!(config["enable_persistence"], true);
    assert_eq!(config["recreate_database_on_startup"], false);
    let startup = config["traqula_file_to_run_on_startup"].as_str().unwrap();
    assert!(
        root.join(startup).is_file(),
        "missing startup file {startup}"
    );

    let getting_started = fs::read_to_string(root.join("docs/GETTING_STARTED.md")).unwrap();
    for required in [
        "Native release archive",
        "Start the native server",
        "Make the first query",
        "Confirm persistence",
        "Create a backup",
        "Troubleshooting",
        "Report beta feedback",
    ] {
        assert!(
            getting_started.contains(required),
            "missing section {required}"
        );
    }
}

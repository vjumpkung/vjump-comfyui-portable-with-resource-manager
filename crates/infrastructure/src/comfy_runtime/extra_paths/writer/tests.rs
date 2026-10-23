use super::*;
use application::comfy_runtime::{ExtraModelPaths, ModelRoot};

const HAND_WRITTEN: &str =
    "# my hand-written file\nlegacy:\n  base_path: D:/legacy\n  checkpoints: ckpts\n";

struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("install");
    fs::create_dir_all(root.join("ComfyUI/models/checkpoints")).unwrap();
    Fixture { dir, root }
}

impl Fixture {
    fn base(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn file(&self) -> PathBuf {
        self.root.join("ComfyUI/extra_model_paths.yaml")
    }

    fn document(&self) -> Mapping {
        let text = fs::read_to_string(self.file()).unwrap();
        serde_yaml_ng::from_str::<Value>(&text)
            .unwrap()
            .as_mapping()
            .unwrap()
            .clone()
    }

    fn section(&self) -> Mapping {
        self.document()
            .get(APP_SECTION)
            .unwrap()
            .as_mapping()
            .unwrap()
            .clone()
    }

    fn backups(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.root.join("data/backups")) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
        paths.sort();
        paths
    }
}

fn text(mapping: &Mapping, key: &str) -> String {
    mapping.get(key).unwrap().as_str().unwrap().to_owned()
}

fn rejection(result: Result<(), RuntimeError>) -> String {
    match result {
        Err(RuntimeError::ExtraPathsRejected(message)) => message,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

fn keys(mapping: &Mapping) -> Vec<String> {
    mapping
        .keys()
        .map(|key| key.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn generate_writes_a_managed_section_and_creates_missing_folders() {
    let fx = fixture();
    let models = fx.root.join("ComfyUI/models");
    // `Loras` differs from the primary `loras` only by case; `clip`, `unet` and hidden folders
    // are never keys; a plain file is not a folder.
    for folder in ["custom_pack", "Loras", "clip", "unet", ".cache"] {
        fs::create_dir(models.join(folder)).unwrap();
    }
    fs::write(models.join("README.txt"), "not a folder").unwrap();
    let base = fx.base("models base");

    // Trailing and forward separators are normalized away before the path is stored.
    let typed = format!("{}/", base.to_str().unwrap().replace('\\', "/"));
    generate(&fx.root, Path::new(&typed)).unwrap();

    for name in PRIMARY {
        assert!(base.join(name).is_dir(), "{name} must be created");
    }
    let file = fs::read_to_string(fx.file()).unwrap();
    assert!(file.starts_with(
        "# Managed by vjumpkung ComfyUI desktop. Other sections are kept; the vjumpkung_desktop section is rewritten by the app.\n"
    ));
    let section = fx.section();
    assert_eq!(text(&section, "base_path"), base.to_str().unwrap());
    assert_eq!(section.get("is_default"), Some(&Value::Bool(false)));
    let mut expected: Vec<String> = PRIMARY.iter().map(|name| (*name).to_owned()).collect();
    expected.extend(["configs".to_owned(), "custom_pack".to_owned()]);
    expected.sort();
    let mut actual = keys(&section);
    assert_eq!(
        actual.drain(..2).collect::<Vec<_>>(),
        ["base_path", "is_default"]
    );
    assert_eq!(actual, expected);
    for name in &expected {
        assert_eq!(text(&section, name), format!("{name}/"));
    }
    // Every folder the section lists exists, including `configs` and the discovered ones.
    for key in keys(&section)
        .iter()
        .filter(|key| !matches!(key.as_str(), "base_path" | "is_default"))
    {
        assert!(base.join(key).is_dir(), "{key} must be created");
    }
}

#[test]
fn generate_refuses_a_file_named_like_a_discovered_category() {
    let fx = fixture();
    fs::create_dir(fx.root.join("ComfyUI/models/custom_pack")).unwrap();
    fs::write(fx.file(), HAND_WRITTEN).unwrap();
    let base = fx.base("models");
    fs::create_dir(&base).unwrap();
    fs::write(base.join("custom_pack"), "a file").unwrap();

    assert!(rejection(generate(&fx.root, &base)).starts_with("A file named custom_pack exists"));

    // Nothing was created and the existing file was neither rewritten nor backed up.
    assert!(!base.join("checkpoints").exists());
    assert!(!base.join("configs").exists());
    assert_eq!(fs::read(fx.file()).unwrap(), HAND_WRITTEN.as_bytes());
    assert!(fx.backups().is_empty());
}

#[test]
fn generate_preserves_other_sections_and_backs_up_a_hand_written_file() {
    let fx = fixture();
    fs::write(fx.file(), HAND_WRITTEN).unwrap();

    generate(&fx.root, &fx.base("models")).unwrap();

    let backups = fx.backups();
    assert_eq!(backups.len(), 1);
    let name = backups[0].file_name().unwrap().to_str().unwrap().to_owned();
    let stamp = name
        .strip_prefix("extra_model_paths-")
        .and_then(|rest| rest.strip_suffix(".yaml"))
        .unwrap();
    assert_eq!(stamp.len(), 16, "{name}");
    assert!(
        stamp.ends_with('Z') && stamp.as_bytes()[8] == b'T',
        "{name}"
    );
    assert_eq!(fs::read(&backups[0]).unwrap(), HAND_WRITTEN.as_bytes());

    let document = fx.document();
    assert_eq!(keys(&document), ["legacy", APP_SECTION]);
    let legacy = document.get("legacy").unwrap().as_mapping().unwrap();
    assert_eq!(text(legacy, "base_path"), "D:/legacy");
    assert_eq!(text(legacy, "checkpoints"), "ckpts");
    // The backup lives under data/backups; nothing is left beside the original, which would
    // dirty the ComfyUI checkout.
    let mut beside: Vec<String> = fs::read_dir(fx.root.join("ComfyUI"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    beside.sort();
    assert_eq!(beside, ["extra_model_paths.yaml", "models"]);
}

#[test]
fn regenerate_is_idempotent_and_makes_no_new_backup() {
    let fx = fixture();
    let base = fx.base("models");
    generate(&fx.root, &base).unwrap();
    assert!(
        !fx.root.join("data/backups").exists(),
        "a fresh file needs no backup"
    );

    fs::write(fx.file(), HAND_WRITTEN).unwrap();
    generate(&fx.root, &base).unwrap();
    let first = fs::read(fx.file()).unwrap();
    assert_eq!(fx.backups().len(), 1);

    generate(&fx.root, &base).unwrap();
    assert_eq!(fs::read(fx.file()).unwrap(), first);
    generate(&fx.root, &fx.base("other base")).unwrap();
    assert_eq!(
        text(&fx.section(), "base_path"),
        fx.base("other base").to_str().unwrap()
    );
    assert_eq!(fx.backups().len(), 1, "the app wrote the file last");
}

#[test]
fn remove_deletes_only_the_app_section_and_the_file_when_empty() {
    let fx = fixture();
    let base = fx.base("models");

    // Nothing to remove: no file, then a file without the section.
    remove(&fx.root).unwrap();
    fs::write(fx.file(), HAND_WRITTEN).unwrap();
    remove(&fx.root).unwrap();
    assert_eq!(fs::read_to_string(fx.file()).unwrap(), HAND_WRITTEN);
    assert!(fx.backups().is_empty());

    // Another section survives; the folders are never deleted.
    generate(&fx.root, &base).unwrap();
    remove(&fx.root).unwrap();
    let document = fx.document();
    assert_eq!(keys(&document), ["legacy"]);
    assert!(base.join("checkpoints").is_dir());

    // The last section takes the file with it.
    fs::remove_file(fx.file()).unwrap();
    generate(&fx.root, &base).unwrap();
    assert!(fx.file().is_file());
    remove(&fx.root).unwrap();
    assert!(!fx.file().exists());
    assert!(base.join("checkpoints").is_dir());
}

#[test]
fn legacy_alias_lines_are_added_only_when_the_folder_exists() {
    let fx = fixture();
    let with_aliases = fx.base("with aliases");
    for alias in ["clip", "unet"] {
        fs::create_dir_all(with_aliases.join(alias)).unwrap();
    }

    generate(&fx.root, &with_aliases).unwrap();
    let section = fx.section();
    assert_eq!(text(&section, "text_encoders"), "text_encoders/\nclip/");
    assert_eq!(
        text(&section, "diffusion_models"),
        "diffusion_models/\nunet/"
    );
    assert_eq!(text(&section, "vae"), "vae/");

    generate(&fx.root, &fx.base("without aliases")).unwrap();
    let section = fx.section();
    assert_eq!(text(&section, "text_encoders"), "text_encoders/");
    assert_eq!(text(&section, "diffusion_models"), "diffusion_models/");
}

#[test]
fn generate_refuses_unsafe_base_paths() {
    let fx = fixture();
    let dir = fx.dir.path();
    fs::write(dir.join("file.txt"), "a file").unwrap();
    let mut cases: Vec<(PathBuf, &str)> = vec![
        (PathBuf::from("relative/models"), NOT_ABSOLUTE),
        (PathBuf::new(), NOT_ABSOLUTE),
        (PathBuf::from(r"\\?\C:\models"), NOT_ABSOLUTE),
        (PathBuf::from(r"\\.\C:\models"), NOT_ABSOLUTE),
        (dir.join("tab\there"), NOT_ABSOLUTE),
        (dir.join("x").join("..").join("y"), NOT_ABSOLUTE),
        (dir.join("a$b"), EXPANDED),
        (dir.join("100%"), EXPANDED),
        (dir.join("%TEMP%"), EXPANDED),
        (PathBuf::from("~/models"), EXPANDED),
        (fx.root.clone(), INSIDE_APP),
        (fx.root.join("ComfyUI"), INSIDE_APP),
        (fx.root.join("ComfyUI/models/new"), INSIDE_APP),
        (fx.root.join("envs/a"), INSIDE_APP),
        (fx.root.join("data"), INSIDE_APP),
        (fx.root.join("tools/x/y"), INSIDE_APP),
        (dir.join("file.txt"), NOT_A_FOLDER),
        (dir.join("file.txt").join("sub"), NOT_A_FOLDER),
    ];
    if cfg!(windows) {
        cases.push((fx.root.join("ENVS").join("x"), INSIDE_APP));
        cases.push((fx.root.join("COMFYUI").join("models"), INSIDE_APP));
    }
    for (base, message) in cases {
        assert_eq!(
            rejection(generate(&fx.root, &base)),
            message,
            "{}",
            base.display()
        );
        assert!(!fx.file().exists(), "{}", base.display());
        assert!(!fx.root.join("data").exists(), "{}", base.display());
    }
    assert!(!fx.root.join("ComfyUI/models/new").exists());
    assert!(!fx.root.join("envs").exists());

    // Not installed: there is no ComfyUI folder to hold the file.
    let empty = dir.join("empty root");
    assert_eq!(
        rejection(generate(&empty, &dir.join("models"))),
        NOT_INSTALLED
    );
    assert!(!dir.join("models").exists());

    // A file with a category's name refuses before any folder is created.
    let blocked = dir.join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("loras"), "a file").unwrap();
    assert!(rejection(generate(&fx.root, &blocked)).starts_with("A file named loras exists"));
    assert!(!blocked.join("checkpoints").exists());
    assert!(!fx.file().exists());
}

#[test]
fn generate_refuses_a_file_it_cannot_rewrite_safely() {
    let fx = fixture();
    let base = fx.base("models");
    let big = vec![b'#'; (1 << 20) + 1];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("it is larger than 1 MiB", big),
        ("it is not valid UTF-8", vec![0xff, 0xfe, 0x00]),
        ("it is not valid YAML", b"a: [1\n".to_vec()),
        (
            "its top level is not a mapping of sections",
            b"- a\n- b\n".to_vec(),
        ),
        (
            "it uses a YAML merge key (<<)",
            b"shared: &s\n  loras: loras\nother:\n  <<: *s\n".to_vec(),
        ),
    ];
    for (reason, content) in cases {
        fs::write(fx.file(), &content).unwrap();
        for message in [
            rejection(generate(&fx.root, &base)),
            rejection(remove(&fx.root)),
        ] {
            assert_eq!(
                message,
                format!(
                    "extra_model_paths.yaml could not be updated safely: {reason}. Edit or move it, then retry."
                )
            );
        }
        assert_eq!(fs::read(fx.file()).unwrap(), content, "{reason}");
        assert!(!base.exists(), "{reason}");
        assert!(fx.backups().is_empty(), "{reason}");
    }

    // Not a regular file: a folder in its place.
    fs::remove_file(fx.file()).unwrap();
    fs::create_dir(fx.file()).unwrap();
    assert!(rejection(generate(&fx.root, &base)).contains("it is a link or not a regular file"));
    assert!(!base.exists());
    fs::remove_dir(fx.file()).unwrap();

    // An empty or comment-only file is an empty mapping and is backed up before the rewrite.
    for content in ["", "# only a comment\n"] {
        fs::write(fx.file(), content).unwrap();
        generate(&fx.root, &base).unwrap();
        assert!(
            fx.backups()
                .iter()
                .any(|backup| fs::read(backup).unwrap() == content.as_bytes()),
            "{content:?} must be backed up"
        );
        assert_eq!(keys(&fx.document()), [APP_SECTION]);
    }
}

#[tokio::test]
async fn generated_file_round_trips_through_the_reader() {
    let fx = fixture();
    fs::create_dir(fx.root.join("ComfyUI/models/custom_pack")).unwrap();
    let base = fx.base("แบบจำลอง models");
    for alias in ["clip", "unet"] {
        fs::create_dir_all(base.join(alias)).unwrap();
    }

    YamlExtraModelPaths.generate(&fx.root, &base).await.unwrap();

    let mut names: Vec<&str> = PRIMARY.to_vec();
    names.extend(["configs", "custom_pack"]);
    names.sort_unstable();
    let mut expected = Vec::new();
    for name in names {
        let aliases: &[&str] = match name {
            "text_encoders" => &["clip"],
            "diffusion_models" => &["unet"],
            _ => &[],
        };
        for folder in std::iter::once(name).chain(aliases.iter().copied()) {
            expected.push(ModelRoot {
                section: APP_SECTION.to_owned(),
                category: name.to_owned(),
                path: base.join(folder),
                is_default: false,
            });
        }
    }
    assert_eq!(YamlExtraModelPaths.read(&fx.root).await.unwrap(), expected);

    YamlExtraModelPaths.remove(&fx.root).await.unwrap();
    assert!(YamlExtraModelPaths.read(&fx.root).await.unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn generate_refuses_a_junction_in_the_base_prefix() {
    let fx = fixture();
    let target = fx.base("real");
    let link = fx.base("link");
    fs::create_dir(&target).unwrap();
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output();
    if !made.is_ok_and(|output| output.status.success()) {
        eprintln!("skipped: junctions cannot be created here");
        return;
    }

    for base in [link.join("models"), link.clone()] {
        assert_eq!(rejection(generate(&fx.root, &base)), LINKED);
    }
    assert!(!target.join("models").exists());
    assert!(!fx.file().exists());
}

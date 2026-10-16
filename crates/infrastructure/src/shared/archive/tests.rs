use std::io::{Cursor, Write};

use flate2::{Compression, write::GzEncoder};
use zip::write::SimpleFileOptions;

use super::*;

fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        if name.ends_with('/') {
            writer.add_directory(*name, stored()).unwrap();
        } else {
            writer.start_file(*name, stored()).unwrap();
            writer.write_all(data).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

struct Sandbox {
    _root: tempfile::TempDir,
    archive: PathBuf,
    staging: PathBuf,
}

impl Sandbox {
    fn new(archive_name: &str, bytes: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join(archive_name);
        fs::write(&archive, bytes).unwrap();
        // Spaces and non-ASCII characters in every path component.
        let staging = root.path().join("stage ไทย dir").join("fresh");
        Self {
            _root: root,
            archive,
            staging,
        }
    }

    fn extract(
        &self,
        kind: ArchiveKind,
        limits: ExtractLimits,
    ) -> Result<Vec<PathBuf>, ArchiveError> {
        extract(&self.archive, kind, &self.staging, limits)
    }

    fn assert_clean(&self) {
        let parent = self.staging.parent().unwrap();
        let outside: Vec<_> = fs::read_dir(parent)
            .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
            .unwrap_or_default();
        assert!(
            outside.iter().all(|name| name == "fresh"),
            "something was written beside the staging directory: {outside:?}"
        );
        assert!(
            !self._root.path().join("evil.txt").exists(),
            "an entry escaped the staging directory"
        );
        if self.staging.exists() {
            assert_eq!(
                fs::read_dir(&self.staging).unwrap().count(),
                0,
                "a rejected zip must not write anything"
            );
        }
    }
}

#[test]
fn a_valid_zip_extracts_into_the_fresh_staging_directory() {
    let bytes = zip_bytes(&[
        ("uv.exe", b"binary"),
        ("nested/dir/", b""),
        ("nested/dir/file ไทย.txt", b"content"),
    ]);
    let sandbox = Sandbox::new("tool.zip", &bytes);
    let mut files = sandbox
        .extract(ArchiveKind::Zip, ExtractLimits::default())
        .unwrap();
    files.sort();
    assert_eq!(
        files,
        [
            PathBuf::from("nested").join("dir").join("file ไทย.txt"),
            PathBuf::from("uv.exe")
        ]
    );
    assert_eq!(fs::read(sandbox.staging.join("uv.exe")).unwrap(), b"binary");
    assert_eq!(
        fs::read_to_string(sandbox.staging.join("nested/dir/file ไทย.txt")).unwrap(),
        "content"
    );
}

#[test]
fn malicious_entry_names_are_rejected() {
    for name in [
        "../evil.txt",
        "a/../../evil.txt",
        "a\\..\\..\\evil.txt",
        "/abs/evil.txt",
        "\\abs\\evil.txt",
        "\\\\server\\share\\evil.txt",
        "C:evil.txt",
        "C:\\evil.txt",
        "c:/evil.txt",
        "file.txt:stream",
        "dir/file:Zone.Identifier",
        "CON",
        "dir/nul.txt",
        "COM1.log",
        "lpt9",
        "con.d/x",
        "trailing.",
        "trailing ",
        "bad<name",
        "bad|name",
        "bad?name",
        "ctrl\u{1}char",
        "nul\0byte",
        "",
        ".",
        "./",
    ] {
        assert!(
            validate_entry_name(name).is_err(),
            "{name:?} must be rejected"
        );
    }
    let long = "a".repeat(256);
    assert!(validate_entry_name(&long).is_err());
    for name in [
        "tool.exe",
        "dir/sub/file.txt",
        "./dir/file",
        "ไทย/ไฟล์.txt",
        "dir\\file",
    ] {
        assert!(
            validate_entry_name(name).is_ok(),
            "{name:?} must be accepted"
        );
    }
}

#[test]
fn zips_with_traversal_or_reserved_names_write_nothing() {
    for bad in [
        "../evil.txt",
        "/evil.txt",
        "C:evil.txt",
        "good/../../evil.txt",
        "stream:name",
        "CON.txt",
    ] {
        let bytes = zip_bytes(&[("ok.txt", b"fine"), (bad, b"x")]);
        let sandbox = Sandbox::new("bad.zip", &bytes);
        assert!(
            sandbox
                .extract(ArchiveKind::Zip, ExtractLimits::default())
                .is_err(),
            "{bad:?}"
        );
        sandbox.assert_clean();
    }
}

#[test]
fn zip_symlinks_are_rejected() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("ok.txt", stored()).unwrap();
    writer.write_all(b"fine").unwrap();
    writer
        .add_symlink("link", "../../etc/passwd", stored())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let sandbox = Sandbox::new("link.zip", &bytes);
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
        Err(ArchiveError::UnsafeEntry("symbolic link"))
    );
    sandbox.assert_clean();
}

#[test]
fn zip_reparse_attributes_and_raw_nul_names_are_rejected_before_writing() {
    for attack in ["reparse", "nul"] {
        let mut bytes = zip_bytes(&[("ok.txt", b"fine"), ("bad.txt", b"bad")]);
        let headers: Vec<_> = bytes
            .windows(4)
            .enumerate()
            .filter_map(|(index, signature)| (signature == b"PK\x01\x02").then_some(index))
            .collect();
        let header = headers[1];
        if attack == "reparse" {
            let mut attributes =
                u32::from_le_bytes(bytes[header + 38..header + 42].try_into().unwrap());
            attributes |= 0x400;
            bytes[header + 38..header + 42].copy_from_slice(&attributes.to_le_bytes());
        } else {
            bytes[header + 46 + 3] = 0;
        }
        let sandbox = Sandbox::new("attack.zip", &bytes);
        assert!(
            sandbox
                .extract(ArchiveKind::Zip, ExtractLimits::default())
                .is_err(),
            "{attack}"
        );
        sandbox.assert_clean();
    }
}

#[test]
fn a_high_ratio_entry_cannot_hide_behind_an_incompressible_member() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("padding", stored()).unwrap();
    writer.write_all(&vec![1u8; 2 << 20]).unwrap();
    writer
        .start_file(
            "bomb",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0u8; 1 << 20]).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let sandbox = Sandbox::new("padded-bomb.zip", &bytes);
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
        Err(ArchiveError::CompressionRatio)
    );
    sandbox.assert_clean();
}

#[test]
fn case_insensitive_collisions_and_file_directory_clashes_are_rejected() {
    for entries in [
        vec![("A.txt", &b"1"[..]), ("a.txt", b"2")],
        vec![("dir/", b""), ("DIR/file", b"1"), ("Dir", b"2")],
        vec![("name", b"1"), ("name/inner", b"2")],
    ] {
        let sandbox = Sandbox::new("collide.zip", &zip_bytes(&entries));
        assert_eq!(
            sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
            Err(ArchiveError::Collision),
            "{entries:?}"
        );
        sandbox.assert_clean();
    }
}

#[test]
fn entry_count_total_size_and_compression_ratio_are_capped() {
    let many = zip_bytes(&[("a", b"1"), ("b", b"1"), ("c", b"1"), ("d", b"1")]);
    let sandbox = Sandbox::new("many.zip", &many);
    let limits = ExtractLimits {
        max_entries: 3,
        ..ExtractLimits::default()
    };
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, limits),
        Err(ArchiveError::TooManyEntries)
    );
    sandbox.assert_clean();

    let big = zip_bytes(&[("a", &[7u8; 4096])]);
    let sandbox = Sandbox::new("big.zip", &big);
    let limits = ExtractLimits {
        max_total_bytes: 1024,
        ..ExtractLimits::default()
    };
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, limits),
        Err(ArchiveError::TooLarge)
    );
    sandbox.assert_clean();

    // A zip bomb: a megabyte of zeros deflates to about a kilobyte.
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "zeros.bin",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0u8; 8 << 20]).unwrap();
    let bomb = writer.finish().unwrap().into_inner();
    let sandbox = Sandbox::new("bomb.zip", &bomb);
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
        Err(ArchiveError::CompressionRatio)
    );
    sandbox.assert_clean();
}

#[test]
fn the_staging_directory_must_be_fresh() {
    let sandbox = Sandbox::new("ok.zip", &zip_bytes(&[("a.txt", b"1")]));
    fs::create_dir_all(&sandbox.staging).unwrap();
    fs::write(sandbox.staging.join("existing"), b"keep").unwrap();
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
        Err(ArchiveError::StagingNotFresh)
    );
    assert_eq!(fs::read(sandbox.staging.join("existing")).unwrap(), b"keep");
}

#[test]
fn corrupt_archives_are_rejected() {
    let sandbox = Sandbox::new("junk.zip", b"this is not a zip");
    assert_eq!(
        sandbox.extract(ArchiveKind::Zip, ExtractLimits::default()),
        Err(ArchiveError::Corrupt)
    );
    let sandbox = Sandbox::new("junk.tar.gz", b"this is not a tarball");
    assert!(
        sandbox
            .extract(ArchiveKind::TarGz, ExtractLimits::default())
            .is_err()
    );
}

// ------------------------------------------------------------------------------- tar.gz

fn raw_entry(
    builder: &mut tar::Builder<GzEncoder<Vec<u8>>>,
    name: &str,
    kind: tar::EntryType,
    data: &[u8],
    link: Option<&str>,
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    {
        let gnu = header.as_gnu_mut().unwrap();
        gnu.name[..name.len()].copy_from_slice(name.as_bytes());
    }
    header.set_size(data.len() as u64);
    header.set_entry_type(kind);
    header.set_mode(mode);
    if let Some(link) = link {
        header.set_link_name(link).unwrap();
    }
    header.set_cksum();
    builder.append(&header, data).unwrap();
}

fn tar_gz(build: impl FnOnce(&mut tar::Builder<GzEncoder<Vec<u8>>>)) -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    build(&mut builder);
    builder.into_inner().unwrap().finish().unwrap()
}

#[test]
fn a_valid_tarball_extracts_and_keeps_only_the_executable_bit() {
    let bytes = tar_gz(|builder| {
        raw_entry(
            builder,
            "uv-x86_64-unknown-linux-gnu/",
            tar::EntryType::Directory,
            b"",
            None,
            0o755,
        );
        raw_entry(
            builder,
            "uv-x86_64-unknown-linux-gnu/uv",
            tar::EntryType::Regular,
            b"binary",
            None,
            0o755,
        );
        raw_entry(
            builder,
            "uv-x86_64-unknown-linux-gnu/README",
            tar::EntryType::Regular,
            b"text",
            None,
            0o600,
        );
    });
    let sandbox = Sandbox::new("uv.tar.gz", &bytes);
    let mut files = sandbox
        .extract(ArchiveKind::TarGz, ExtractLimits::default())
        .unwrap();
    files.sort();
    assert_eq!(files.len(), 2);
    assert_eq!(
        fs::read(sandbox.staging.join("uv-x86_64-unknown-linux-gnu/uv")).unwrap(),
        b"binary"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |name: &str| {
            fs::metadata(sandbox.staging.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("uv-x86_64-unknown-linux-gnu/uv"), 0o755);
        assert_eq!(mode("uv-x86_64-unknown-linux-gnu/README"), 0o644);
    }
}

#[test]
fn tar_links_devices_and_traversal_are_rejected() {
    use tar::EntryType::{Block, Char, Fifo, Link, Regular, Symlink};
    let cases: Vec<(&str, tar::EntryType, Option<&str>)> = vec![
        ("link", Symlink, Some("../../etc/passwd")),
        ("hard", Link, Some("ok.txt")),
        ("dev", Char, None),
        ("blk", Block, None),
        ("pipe", Fifo, None),
        ("../evil.txt", Regular, None),
        ("/evil.txt", Regular, None),
        ("a/../../evil.txt", Regular, None),
        ("C:evil.txt", Regular, None),
        ("CON", Regular, None),
    ];
    for (name, kind, link) in cases {
        let bytes = tar_gz(|builder| {
            raw_entry(builder, "ok.txt", Regular, b"fine", None, 0o644);
            raw_entry(builder, name, kind, b"", link, 0o644);
        });
        let sandbox = Sandbox::new("bad.tar.gz", &bytes);
        let result = sandbox.extract(ArchiveKind::TarGz, ExtractLimits::default());
        assert!(result.is_err(), "{name:?} {kind:?} must be rejected");
        assert!(!sandbox._root.path().join("evil.txt").exists());
        assert!(!sandbox.staging.parent().unwrap().join("evil.txt").exists());
    }
}

#[test]
fn tar_collisions_and_limits_are_enforced() {
    let bytes = tar_gz(|builder| {
        raw_entry(builder, "A", tar::EntryType::Regular, b"1", None, 0o644);
        raw_entry(builder, "a", tar::EntryType::Regular, b"2", None, 0o644);
    });
    let sandbox = Sandbox::new("collide.tar.gz", &bytes);
    assert_eq!(
        sandbox.extract(ArchiveKind::TarGz, ExtractLimits::default()),
        Err(ArchiveError::Collision)
    );

    let bytes = tar_gz(|builder| {
        for name in ["a", "b", "c"] {
            raw_entry(builder, name, tar::EntryType::Regular, b"x", None, 0o644);
        }
    });
    let sandbox = Sandbox::new("many.tar.gz", &bytes);
    let limits = ExtractLimits {
        max_entries: 2,
        ..ExtractLimits::default()
    };
    assert_eq!(
        sandbox.extract(ArchiveKind::TarGz, limits),
        Err(ArchiveError::TooManyEntries)
    );

    let bytes = tar_gz(|builder| {
        raw_entry(
            builder,
            "zeros",
            tar::EntryType::Regular,
            &vec![0u8; 4 << 20],
            None,
            0o644,
        );
    });
    let sandbox = Sandbox::new("bomb.tar.gz", &bytes);
    assert_eq!(
        sandbox.extract(ArchiveKind::TarGz, ExtractLimits::default()),
        Err(ArchiveError::CompressionRatio)
    );
    let limits = ExtractLimits {
        max_total_bytes: 1 << 20,
        max_ratio: u64::MAX,
        ..ExtractLimits::default()
    };
    let sandbox = Sandbox::new(
        "big.tar.gz",
        &tar_gz(|builder| {
            raw_entry(
                builder,
                "zeros",
                tar::EntryType::Regular,
                &vec![0u8; 4 << 20],
                None,
                0o644,
            );
        }),
    );
    assert_eq!(
        sandbox.extract(ArchiveKind::TarGz, limits),
        Err(ArchiveError::TooLarge)
    );
}

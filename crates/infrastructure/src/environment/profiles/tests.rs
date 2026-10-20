use std::collections::HashSet;

use domain::environment::{
    ComputeCapability, Eligibility, GpuObservation, GpuProbe, HardwareObservation, ProfileStatus,
    PythonVersion, evaluate,
};

use super::*;
use crate::environment::pins;

fn catalog() -> BundledProfiles {
    BundledProfiles::load().expect("every bundled profile must parse and validate")
}

#[test]
fn every_bundled_profile_parses_validates_and_has_a_unique_id() {
    let profiles = catalog().list();
    assert_eq!(profiles.len(), 7);
    let ids: HashSet<_> = profiles.iter().map(|profile| profile.id.clone()).collect();
    assert_eq!(ids.len(), profiles.len());
    for profile in &profiles {
        profile.validate().unwrap();
        assert!(!profile.provenance.is_empty(), "{}", profile.id);
        // Only the Python 3.13 Windows profile was measured on a real install.
        assert_eq!(
            profile.disk.calibrated,
            profile.id == "win-py313-torch2141-cu130",
            "{}",
            profile.id
        );
        if profile.id == "win-py312-torch2100-cu130" {
            assert_eq!(profile.revision, 2);
            assert_eq!(profile.accelerators.len(), 6);
            assert!(
                profile
                    .accelerators
                    .iter()
                    .all(|a| !a.default_selected && a.non_fatal)
            );
            assert!(
                profile
                    .accelerators
                    .iter()
                    .all(|a| a.no_deps == (a.id == "onnxruntime-gpu"))
            );
        } else {
            assert!(profile.accelerators.is_empty());
            assert_eq!(profile.revision, 1);
        }
        // The id encodes OS, Python, torch and CUDA.
        let os = match profile.os {
            domain::environment::OsKind::Windows => "win",
            domain::environment::OsKind::Linux => "linux",
        };
        assert!(profile.id.starts_with(os), "{}", profile.id);
        assert!(
            profile.id.ends_with(profile.torch.cuda.as_str()),
            "{}",
            profile.id
        );
        assert!(profile.id.contains(&format!(
            "py{}{}",
            profile.python.major, profile.python.minor
        )));
        let digits = profile.torch.torch.replace('.', "");
        assert!(
            profile.id.contains(&format!("torch{digits}")),
            "{}",
            profile.id
        );
    }
}

#[test]
fn each_os_has_exactly_one_recommended_profile_with_the_spec_defaults() {
    let profiles = catalog().list();
    // Windows recommends Python 3.12 with torch 2.10 (owner decision 2026-10-20).
    for (os, id, python) in [
        (
            domain::environment::OsKind::Windows,
            "win-py312-torch2100-cu130",
            PythonVersion::new(3, 12, 15),
        ),
        (
            domain::environment::OsKind::Linux,
            "linux-py313-torch2141-cu130",
            PythonVersion::new(3, 13, 16),
        ),
    ] {
        let recommended: Vec<_> = profiles
            .iter()
            .filter(|profile| profile.os == os && profile.recommended)
            .collect();
        assert_eq!(recommended.len(), 1, "{os:?}");
        assert_eq!(recommended[0].id, id);
        assert_eq!(recommended[0].python, python);
    }
}

#[test]
fn the_pinned_stacks_match_the_verified_evidence() {
    let catalog = catalog();
    let torch_2141 = |id: &str| catalog.get(id).unwrap();
    for id in [
        "win-py313-torch2141-cu130",
        "linux-py313-torch2141-cu130",
        "win-py313-torch2141-cu126",
        "linux-py313-torch2141-cu126",
        "win-py314-torch2141-cu130",
        "linux-py314-torch2141-cu130",
    ] {
        let profile = torch_2141(id);
        assert_eq!(profile.torch.torch, "2.14.1", "{id}");
        assert_eq!(profile.torch.torchvision, "0.29.1", "{id}");
        assert_eq!(
            profile.torch.torchaudio, None,
            "{id}: no torchaudio 2.14.x wheel exists"
        );
        assert_eq!(
            profile.min_cc,
            ComputeCapability::new(8, 0),
            "{id}: Triton floor"
        );
        let expected_triton = if id.starts_with("win") {
            "triton-windows"
        } else {
            "triton"
        };
        assert_eq!(
            profile.triton.distribution.as_str(),
            expected_triton,
            "{id}"
        );
        let expected_driver = if id.ends_with("cu126") {
            (12, 6)
        } else {
            (13, 0)
        };
        assert_eq!(
            (profile.min_driver_cuda.major, profile.min_driver_cuda.minor),
            expected_driver,
            "{id}"
        );
        assert_eq!(
            profile.status,
            if id.contains("py314") {
                ProfileStatus::Experimental
            } else {
                ProfileStatus::Stable
            },
            "{id}"
        );
    }
    assert_eq!(
        torch_2141("win-py314-torch2141-cu130").python,
        PythonVersion::new(3, 14, 8)
    );

    let compat = catalog.get("win-py312-torch2100-cu130").unwrap();
    assert_eq!(
        compat.python,
        PythonVersion::new(3, 12, 15),
        "an exact patch, never 3.12.x"
    );
    assert_eq!(
        (
            compat.torch.torch.as_str(),
            compat.torch.torchvision.as_str()
        ),
        ("2.10.0", "0.25.0")
    );
    assert_eq!(compat.torch.torchaudio.as_deref(), Some("2.10.0"));
    assert_eq!(compat.triton.specifier, ">=3.6,<3.7");
    assert_eq!(compat.verify.torch_version_prefix, "2.10.0+cu130");
}

#[test]
fn cuda_126_wheels_do_not_cover_blackwell_and_cuda_130_wheels_do() {
    let catalog = catalog();
    let hardware = |major: u8, minor: u8| HardwareObservation {
        gpus: GpuProbe::Detected(vec![GpuObservation {
            index: 0,
            name: "GPU".into(),
            compute_capability: Some(ComputeCapability::new(major, minor)),
            vram_mib: Some(16384),
        }]),
        driver_version: Some("999".into()),
        driver_cuda: Some(domain::environment::CudaVersion::new(13, 2)),
        os: domain::environment::OsKind::Windows,
        os_version: "Windows".into(),
        install_volume_free: Some(1 << 40),
        env_volume_free: None,
        env_on_separate_volume: false,
        long_paths_enabled: Some(true),
    };
    let cu126 = catalog.get("win-py313-torch2141-cu126").unwrap();
    let cu130 = catalog.get("win-py313-torch2141-cu130").unwrap();
    // RTX 3060 (8.6) and Hopper (9.0) work with both; Blackwell (12.0) only with CUDA 13.
    for (major, minor, ok_126, ok_130) in [
        (7, 5, false, false),
        (8, 0, true, true),
        (8, 6, true, true),
        (9, 0, true, true),
        (12, 0, false, true),
    ] {
        assert_eq!(
            evaluate(&cu126, &hardware(major, minor)) == Eligibility::Eligible,
            ok_126,
            "cu126 {major}.{minor}"
        );
        assert_eq!(
            evaluate(&cu130, &hardware(major, minor)) == Eligibility::Eligible,
            ok_130,
            "cu130 {major}.{minor}"
        );
    }
}

#[test]
fn managed_pythons_are_pinned_with_sha256_in_the_download_metadata() {
    let downloads: serde_json::Value = serde_json::from_str(pins::PYTHON_DOWNLOADS_JSON).unwrap();
    let entries = downloads.as_object().unwrap();
    assert_eq!(entries.len(), 6);
    let mut covered = Vec::new();
    for profile in catalog().list() {
        let key = format!(
            "cpython-{}-{}",
            profile.python,
            match profile.os {
                domain::environment::OsKind::Windows => "windows-x86_64-none",
                domain::environment::OsKind::Linux => "linux-x86_64-gnu",
            }
        );
        let entry = entries
            .get(&key)
            .unwrap_or_else(|| panic!("{key} is missing from python-downloads.json"));
        let sha = entry["sha256"].as_str().unwrap();
        assert_eq!(sha.len(), 64);
        assert!(sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(entry["url"].as_str().unwrap().starts_with(
            "https://github.com/astral-sh/python-build-standalone/releases/download/"
        ));
        covered.push(key);
    }
    // The hf tool Python is pinned on both platforms too.
    for os in ["windows-x86_64-none", "linux-x86_64-gnu"] {
        assert!(entries.contains_key(&format!("cpython-{}-{os}", pins::HF_TOOL_PYTHON)));
    }
    assert!(!covered.is_empty());
}

#[test]
fn documents_round_trip_and_reject_unknown_or_unsupported_content() {
    for profile in catalog().list() {
        let doc = ProfileDoc::from(&profile);
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(parse_profile(&json).unwrap(), profile);
    }
    let base = serde_json::to_value(ProfileDoc::from(
        &catalog().get("win-py313-torch2141-cu130").unwrap(),
    ))
    .unwrap();
    let mutate = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut value = base.clone();
        change(&mut value);
        parse_profile(&value.to_string())
    };
    assert!(mutate(&|v| v["unexpected"] = true.into()).is_err());
    assert!(mutate(&|v| v["schema_version"] = 3.into()).is_err());
    assert!(mutate(&|v| v["python"] = "3.13.x".into()).is_err());
    assert!(mutate(&|v| v["python"] = "3.13".into()).is_err());
    assert!(mutate(&|v| v["status"] = "beta".into()).is_err());
    assert!(mutate(&|v| v["os"] = "macos".into()).is_err());
    assert!(
        mutate(&|v| v["torch"]["index_url"] = "http://download.pytorch.org/whl/cu130".into())
            .is_err()
    );
    assert!(mutate(&|v| v["triton"]["package"] = "triton*".into()).is_err());
    assert!(mutate(&|v| v["min_cc"] = "eight".into()).is_err());
    assert!(parse_profile("not json").is_err());
}

#[test]
fn tool_pins_are_well_formed() {
    for artifact in [&pins::UV_WINDOWS, &pins::UV_LINUX] {
        assert!(artifact.url.starts_with("https://"));
        assert!(artifact.url.contains(pins::UV_VERSION));
        assert_eq!(artifact.sha256.len(), 64);
        assert!(artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert!(pins::MINGIT_URL.starts_with("https://"));
    assert!(pins::MINGIT_URL.contains(pins::MINGIT_VERSION));
    assert_eq!(pins::MINGIT_SHA256.len(), 64);
    // The hash-locked requirement files pin hf exactly and require hashes everywhere.
    for requirements in [pins::HF_REQUIREMENTS_WINDOWS, pins::HF_REQUIREMENTS_LINUX] {
        assert!(requirements.contains(&format!("hf=={}", pins::HF_VERSION)));
        assert!(requirements.contains("hf-xet==1.7.0"));
        let packages = requirements
            .lines()
            .filter(|line| line.contains("==") && !line.trim_start().starts_with('#'))
            .count();
        let hashes = requirements.matches("--hash=sha256:").count();
        assert!(
            hashes >= packages,
            "every package carries at least one hash"
        );
        assert!(
            requirements
                .lines()
                .filter(|line| line.contains("==") && !line.trim_start().starts_with('#'))
                .all(|line| line.trim_end().ends_with('\\')),
            "each pinned line is followed by hash lines"
        );
    }
    // The gdown lock pins gdown exactly and requires hashes everywhere.
    for requirements in [
        pins::GDOWN_REQUIREMENTS_WINDOWS,
        pins::GDOWN_REQUIREMENTS_LINUX,
    ] {
        assert!(requirements.contains(&format!("gdown=={}", pins::GDOWN_VERSION)));
        let packages = requirements
            .lines()
            .filter(|line| line.contains("==") && !line.trim_start().starts_with('#'))
            .count();
        let hashes = requirements.matches("--hash=sha256:").count();
        assert!(
            hashes >= packages,
            "every package carries at least one hash"
        );
        assert!(
            requirements
                .lines()
                .filter(|line| line.contains("==") && !line.trim_start().starts_with('#'))
                .all(|line| line.trim_end().ends_with('\\')),
            "each pinned line is followed by hash lines"
        );
    }
}

#[test]
fn accelerator_specs_reject_invalid_artifacts() {
    let profile = catalog().get("win-py312-torch2100-cu130").unwrap();
    let base = serde_json::to_value(ProfileDoc::from(&profile)).unwrap();
    let mutations: Vec<(&str, serde_json::Value)> = vec![
        ("hash", serde_json::json!("g".repeat(64))),
        ("filename", serde_json::json!("../escape.whl")),
        ("extension", serde_json::json!("source.tar.gz")),
        ("module", serde_json::json!(["invalid-module"])),
        ("id", serde_json::json!("Upper ID")),
        ("provenance", serde_json::json!(" ")),
    ];
    for (case, value) in mutations {
        let mut doc = base.clone();
        match case {
            "hash" => doc["accelerators"][0]["source"]["sha256"] = value,
            "filename" | "extension" => doc["accelerators"][0]["source"]["filename"] = value,
            "module" => doc["accelerators"][0]["modules"] = value,
            "id" => doc["accelerators"][0]["id"] = value,
            "provenance" => doc["accelerators"][0]["provenance"] = value,
            _ => unreachable!(),
        }
        assert!(parse_profile(&doc.to_string()).is_err(), "{case}");
    }
    for url in [
        "http://example.test/a.whl",
        "https://",
        "https://user:secret@example.test/a.whl",
        "https://@example.test/a.whl",
        "https://example.test/a.whl?token=secret",
        "https://example.test/a.whl?X-Amz-Signature=secret",
        "https://example.test/a.whl?sig=secret",
        "https://example.test/a.whl?api_key=secret",
    ] {
        let mut doc = base.clone();
        doc["accelerators"][0]["source"]["url"] = url.into();
        assert!(parse_profile(&doc.to_string()).is_err(), "{url}");
    }
    let mut doc = base;
    doc["accelerators"][1]["id"] = doc["accelerators"][0]["id"].clone();
    assert!(parse_profile(&doc.to_string()).is_err());
    assert!(
        profile
            .accelerators
            .iter()
            .find(|a| a.id == "flashattention-3")
            .unwrap()
            .modules
            .is_empty()
    );
}

#[test]
fn profile_v1_empty_and_v2_components_round_trip() {
    let old = include_str!("../data/profiles/archive/win-py312-torch2100-cu130-r1.json");
    let legacy = parse_profile(old).unwrap();
    assert_eq!(legacy.schema_version, 1);
    assert!(legacy.accelerators.is_empty());
    let current = catalog().get("win-py312-torch2100-cu130").unwrap();
    for profile in [legacy, current] {
        let doc = serde_json::to_string(&ProfileDoc::from(&profile)).unwrap();
        assert_eq!(parse_profile(&doc).unwrap(), profile);
    }
    let mut value: serde_json::Value = serde_json::from_str(old).unwrap();
    value["accelerators"] = serde_json::json!([{"name":"legacy", "requirement":"x", "sha256":"0".repeat(64), "non_fatal":true}]);
    assert!(parse_profile(&value.to_string()).is_err());
    assert_eq!(
        catalog()
            .get_revision("win-py312-torch2100-cu130", 1)
            .unwrap()
            .revision,
        1
    );
}

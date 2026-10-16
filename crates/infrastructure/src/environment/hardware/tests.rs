use super::*;

#[test]
fn gpu_csv_is_parsed_including_unknown_fields() {
    let text = "0, NVIDIA GeForce RTX 3060, 591.86, 8.6, 12288\n1, Tesla T4, 591.86, [N/A], 15360\n\ngarbage line\nx, bad index, 1, 8.0, 1\n";
    let reading = parse_gpu_csv(text);
    assert_eq!(reading.driver_version.as_deref(), Some("591.86"));
    assert_eq!(reading.gpus.len(), 2);
    assert_eq!(reading.gpus[0].name, "NVIDIA GeForce RTX 3060");
    assert_eq!(
        reading.gpus[0].compute_capability,
        Some(ComputeCapability::new(8, 6))
    );
    assert_eq!(reading.gpus[0].vram_mib, Some(12288));
    assert_eq!(
        reading.gpus[1].compute_capability, None,
        "unknown stays unknown"
    );
    assert_eq!(parse_gpu_csv("").gpus.len(), 0);
}

#[test]
fn the_cuda_version_comes_from_the_header() {
    let header = "| NVIDIA-SMI 591.86   Driver Version: 591.86   CUDA Version: 13.1     |";
    assert_eq!(parse_cuda_header(header), Some(CudaVersion::new(13, 1)));
    assert_eq!(
        parse_cuda_header("CUDA Version: 12.6"),
        Some(CudaVersion::new(12, 6))
    );
    assert_eq!(parse_cuda_header("no cuda here"), None);
    assert_eq!(parse_cuda_header("CUDA Version: N/A"), None);
}

fn volumes() -> Vec<Volume> {
    let make = |mount: &str, available: u64| Volume {
        mount: PathBuf::from(mount),
        available,
    };
    if cfg!(windows) {
        vec![make(r"C:\", 10), make(r"D:\", 20), make(r"D:\data", 30)]
    } else {
        vec![
            make("/", 10),
            make("/mnt/data", 20),
            make("/mnt/data/fast", 30),
        ]
    }
}

#[test]
fn the_longest_matching_mount_point_decides_the_volume() {
    let volumes = volumes();
    let (root, data, fast, nested) = if cfg!(windows) {
        (
            r"C:\Users\me",
            r"D:\ComfyUI",
            r"D:\data\models",
            r"d:\DATA\x",
        )
    } else {
        (
            "/home/me",
            "/mnt/data/ComfyUI",
            "/mnt/data/fast/models",
            "/mnt/data/fast/x",
        )
    };
    assert_eq!(volume_of(Path::new(root), &volumes).unwrap().available, 10);
    assert_eq!(volume_of(Path::new(data), &volumes).unwrap().available, 20);
    assert_eq!(volume_of(Path::new(fast), &volumes).unwrap().available, 30);
    // Windows paths compare case-insensitively; other systems compare exactly.
    assert_eq!(
        volume_of(Path::new(nested), &volumes).unwrap().available,
        30
    );
    // `/mnt/database` must not match the `/mnt/data` mount.
    if !cfg!(windows) {
        assert_eq!(
            volume_of(Path::new("/mnt/database"), &volumes)
                .unwrap()
                .available,
            10
        );
    }
    assert!(volume_of(Path::new("relative"), &[]).is_none());
}

#[tokio::test]
async fn a_real_probe_returns_plausible_values() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install root ไทย");
    let observation = SystemHardware::new().probe(&root, None).await;
    assert_eq!(
        observation.os,
        if cfg!(windows) {
            OsKind::Windows
        } else {
            OsKind::Linux
        }
    );
    assert!(!observation.os_version.is_empty());
    assert!(
        observation.install_volume_free.is_some_and(|free| free > 0),
        "a missing install root resolves to its nearest existing ancestor's volume"
    );
    assert!(!observation.env_on_separate_volume);
    assert_eq!(observation.env_volume_free, None);
    assert_eq!(observation.long_paths_enabled.is_some(), cfg!(windows));
    assert!(!root.exists(), "probing never creates the install root");
    // A GPU result is machine dependent, but it is always one of the three states.
    assert!(matches!(
        observation.gpus,
        GpuProbe::Detected(_) | GpuProbe::NoNvidiaGpu | GpuProbe::Unavailable
    ));
}

#[tokio::test]
async fn an_environment_on_the_same_volume_is_not_reported_as_separate() {
    let directory = tempfile::tempdir().unwrap();
    let observation = SystemHardware::new()
        .probe(
            &directory.path().join("install"),
            Some(&directory.path().join("elsewhere").join("venv")),
        )
        .await;
    assert!(!observation.env_on_separate_volume);
    assert_eq!(observation.env_volume_free, None);
}

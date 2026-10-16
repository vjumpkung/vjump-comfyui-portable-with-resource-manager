//! Pinned tool versions, download URLs and SHA-256 values (NFR-SEC-7).
//!
//! Hash sources, all observed on 2026-10-16:
//! * uv 0.13.1: the `.sha256` assets published beside each archive at
//!   <https://github.com/astral-sh/uv/releases/tag/0.13.1> (also the release API `digest`).
//! * MinGit 2.55.0.3: the SHA-256 table in the release notes of
//!   <https://github.com/git-for-windows/git/releases/tag/v2.55.0.windows.3>; the value matches
//!   the one pinned by the reference installer.
//! * Managed CPython: `python-downloads.json` holds the six entries (3.12.15, 3.13.16 and
//!   3.14.8 for Windows x64 and Linux x64 glibc) copied from uv 0.13.1's
//!   `crates/uv-python-managed/download-metadata.json`. uv verifies the SHA-256 of every
//!   download against that file.

pub const UV_VERSION: &str = "0.13.1";

pub struct ToolArtifact {
    pub url: &'static str,
    pub sha256: &'static str,
    /// The path of the executable inside the archive.
    pub executable: &'static str,
}

pub const UV_WINDOWS: ToolArtifact = ToolArtifact {
    url: "https://github.com/astral-sh/uv/releases/download/0.13.1/uv-x86_64-pc-windows-msvc.zip",
    sha256: "3b2661a463a5baea7f5076ea2b25ab71d77f36a6c0d809e5350651792f80a672",
    executable: "uv.exe",
};

pub const UV_LINUX: ToolArtifact = ToolArtifact {
    url: "https://github.com/astral-sh/uv/releases/download/0.13.1/uv-x86_64-unknown-linux-gnu.tar.gz",
    sha256: "eadd9d27c134ab5ac1ffd4473b97a741407f3ea238288f03121fc4deddf90220",
    executable: "uv-x86_64-unknown-linux-gnu/uv",
};

pub fn uv_artifact() -> &'static ToolArtifact {
    if cfg!(windows) {
        &UV_WINDOWS
    } else {
        &UV_LINUX
    }
}

pub const MINGIT_VERSION: &str = "2.55.0.3";
pub const MINGIT_GIT_VERSION: &str = "2.55.0.windows.3";
pub const MINGIT_URL: &str = "https://github.com/git-for-windows/git/releases/download/v2.55.0.windows.3/MinGit-2.55.0.3-64-bit.zip";
pub const MINGIT_SHA256: &str = "f48e2d2dc74a24454adc6d8fd0ac25bf9c2386f19cfb06202b9465aaad4f9f05";

/// The minimum system Git version on Linux.
pub const MIN_SYSTEM_GIT: (u32, u32) = (2, 40);

pub const PYTHON_DOWNLOADS_JSON: &str = include_str!("data/python-downloads.json");

/// The `hf` tool always uses this managed Python, whatever Python ComfyUI uses.
pub const HF_TOOL_PYTHON: &str = "3.13.16";
pub const HF_VERSION: &str = "2.2.0";

pub const HF_REQUIREMENTS_WINDOWS: &str = include_str!("data/hf-requirements-windows.txt");
pub const HF_REQUIREMENTS_LINUX: &str = include_str!("data/hf-requirements-linux.txt");

pub fn hf_requirements() -> &'static str {
    if cfg!(windows) {
        HF_REQUIREMENTS_WINDOWS
    } else {
        HF_REQUIREMENTS_LINUX
    }
}

/// Windows needs app-local MSVC runtime DLLs next to the venv's `python.exe`.
pub const MSVC_RUNTIME: &str = "msvc-runtime==14.44.35112";

/// The ComfyUI repository and ComfyUI-Manager.
pub const COMFYUI_URL: &str = "https://github.com/Comfy-Org/ComfyUI.git";
pub const MANAGER_URL: &str = "https://github.com/Comfy-Org/ComfyUI-Manager.git";
pub const COMFYUI_RELEASES_API: &str =
    "https://api.github.com/repos/Comfy-Org/ComfyUI/releases/latest";

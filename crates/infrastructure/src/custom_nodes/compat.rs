//! Compatibility observations. SemVer comparison lives here; the domain only receives plain
//! `Support` values. Unparseable or missing metadata is `Unknown`, never `Compatible`.
use domain::custom_nodes::{Compatibility, CompatibilityMetadata, HostOs, Support};
use semver::Version;
use std::path::Path;

/// What this machine and installation look like to a node.
#[derive(Debug, Clone)]
pub struct Host {
    pub os: HostOs,
    /// Every bundled profile installs the NVIDIA CUDA build of torch.
    pub cuda: bool,
    pub comfyui: Option<Version>,
}

impl Host {
    pub fn observe(root: &Path) -> Self {
        Self {
            os: if cfg!(windows) {
                HostOs::Windows
            } else {
                HostOs::Linux
            },
            cuda: true,
            comfyui: comfyui_version(root),
        }
    }
}

/// Reads `__version__` from ComfyUI's `comfyui_version.py` without importing Python.
pub fn comfyui_version(root: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(root.join("ComfyUI").join("comfyui_version.py")).ok()?;
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("__version__")?;
        let value = value.trim_start().strip_prefix('=')?.trim();
        parse_version(value.trim_matches(['"', '\'']))
    })
}

/// Parses `1`, `1.2`, `1.2.3` and `v1.2.3-rc.1` spellings.
pub fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    if text.is_empty() {
        return None;
    }
    let (core, rest) = match text.find(['-', '+']) {
        Some(index) => text.split_at(index),
        None => (text, ""),
    };
    let mut parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    while parts.len() < 3 {
        parts.push("0");
    }
    Version::parse(&format!("{}{rest}", parts.join("."))).ok()
}

/// Evaluates comma-separated comparators such as `>=0.3.0,<1.0`. Only `>= <= > < == =` and
/// `!=` are supported; anything else is unknown.
pub fn comfyui_support(expression: &str, version: &Version) -> Support {
    let mut result = Support::Compatible;
    let mut any = false;
    for clause in expression.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            return Support::Unknown;
        }
        let (op, rest) = ["<=", ">=", "==", "!=", "<", ">", "="]
            .iter()
            .find_map(|op| clause.strip_prefix(op).map(|rest| (*op, rest)))
            .unwrap_or(("", clause));
        if op.is_empty() && rest == clause && !rest.starts_with(|c: char| c.is_ascii_digit()) {
            return Support::Unknown;
        }
        let Some(target) = parse_version(rest) else {
            return Support::Unknown;
        };
        let ok = match op {
            ">=" => version >= &target,
            "<=" => version <= &target,
            ">" => version > &target,
            "<" => version < &target,
            "!=" => version != &target,
            _ => version == &target,
        };
        any = true;
        if !ok {
            result = Support::Incompatible;
        }
    }
    if any { result } else { Support::Unknown }
}

pub fn evaluate(metadata: &CompatibilityMetadata, host: &Host) -> Compatibility {
    let comfyui = match (&metadata.comfyui_expression, &host.comfyui) {
        (Some(expression), Some(version)) => comfyui_support(expression, version),
        _ => Support::Unknown,
    };
    Compatibility {
        os: metadata.os_support(host.os),
        accelerator: metadata.accelerator_support(host.cuda),
        comfyui,
        flagged: metadata.is_flagged(),
        deprecated: metadata.deprecated.unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        parse_version(text).unwrap()
    }

    #[test]
    fn versions_are_padded_and_prefixes_removed() {
        assert_eq!(v("v0.3"), Version::new(0, 3, 0));
        assert_eq!(v("1"), Version::new(1, 0, 0));
        assert!(v("1.2.3-rc.1") < v("1.2.3"));
        for bad in ["", "a.b", "1..2", "1.2.3.4"] {
            assert!(parse_version(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn comfyui_expressions_use_semver_or_report_unknown() {
        assert_eq!(
            comfyui_support(">=0.3.0,<1.0", &v("0.3.50")),
            Support::Compatible
        );
        assert_eq!(
            comfyui_support(">=0.4", &v("0.3.50")),
            Support::Incompatible
        );
        assert_eq!(
            comfyui_support("<0.3.0", &v("0.3.50")),
            Support::Incompatible
        );
        assert_eq!(comfyui_support("~=0.3", &v("0.3.50")), Support::Unknown);
        assert_eq!(comfyui_support("latest", &v("0.3.50")), Support::Unknown);
        assert_eq!(comfyui_support("", &v("0.3.50")), Support::Unknown);
    }

    #[test]
    fn missing_host_version_is_unknown_not_compatible() {
        let metadata = CompatibilityMetadata {
            comfyui_expression: Some(">=0.3".into()),
            ..Default::default()
        };
        let host = Host {
            os: HostOs::Windows,
            cuda: true,
            comfyui: None,
        };
        assert_eq!(evaluate(&metadata, &host).comfyui, Support::Unknown);
    }

    #[test]
    fn the_version_file_is_parsed_as_text() {
        let directory = tempfile::tempdir().unwrap();
        let comfy = directory.path().join("ComfyUI");
        std::fs::create_dir_all(&comfy).unwrap();
        std::fs::write(
            comfy.join("comfyui_version.py"),
            "# comment\n__version__ = \"0.3.50\"\n",
        )
        .unwrap();
        assert_eq!(
            comfyui_version(directory.path()),
            Some(Version::new(0, 3, 50))
        );
        assert_eq!(comfyui_version(&directory.path().join("missing")), None);
    }
}

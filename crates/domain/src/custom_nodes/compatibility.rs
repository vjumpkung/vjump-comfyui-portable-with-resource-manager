#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support { Compatible, Incompatible, Unknown }
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompatibilityMetadata {
    pub supported_os: Option<Vec<String>>,
    pub supported_accelerators: Option<Vec<String>>,
    pub comfyui_expression: Option<String>,
    pub status: Option<String>,
    pub deprecated: Option<bool>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs { Windows, Linux }

impl CompatibilityMetadata {
    /// Recognizes the upstream classifier spellings; missing or unrecognized lists are unknown.
    pub fn os_support(&self, host: HostOs) -> Support {
        let Some(list) = self.supported_os.as_ref().filter(|l| !l.is_empty()) else { return Support::Unknown };
        let mut recognized = false;
        for item in list {
            let item = item.to_ascii_lowercase();
            if item.contains("os independent") { return Support::Compatible; }
            let (windows, linux) = (item.contains("windows"), item.contains("linux") || item.contains("posix"));
            recognized |= windows || linux || item.contains("macos");
            if (windows && host == HostOs::Windows) || (linux && host == HostOs::Linux) { return Support::Compatible; }
        }
        if recognized { Support::Incompatible } else { Support::Unknown }
    }
    pub fn accelerator_support(&self, cuda: bool) -> Support {
        let Some(list) = self.supported_accelerators.as_ref().filter(|l| !l.is_empty()) else { return Support::Unknown };
        let mut recognized = false;
        for item in list {
            let item = item.to_ascii_lowercase();
            if item.contains("cuda") || item.contains("nvidia") { if cuda { return Support::Compatible; } recognized = true; }
            else if item.contains("cpu") || item.contains("rocm") || item.contains("amd") || item.contains("metal") || item.contains("mps") || item.contains("gpu") { recognized = true; }
        }
        if recognized { Support::Incompatible } else { Support::Unknown }
    }
    pub fn is_flagged(&self) -> bool {
        self.status.as_deref().is_some_and(|s| {
            let s = s.to_ascii_lowercase();
            s.contains("banned") || s.contains("deleted") || s.contains("flagged") || s.contains("pending")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compatibility {
    pub os: Support,
    pub accelerator: Support,
    pub comfyui: Support,
    pub flagged: bool,
    pub deprecated: bool,
}
impl Compatibility {
    pub fn warnings(&self) -> Vec<String> {
        let mut result = Vec::new();
        for (name, support) in [("OS", self.os), ("accelerator", self.accelerator), ("ComfyUI", self.comfyui)] {
            match support {
                Support::Incompatible => result.push(format!("{name} is incompatible")),
                Support::Unknown => result.push(format!("{name} compatibility is unknown")),
                Support::Compatible => {}
            }
        }
        if self.flagged { result.push("Registry status is flagged or unavailable".into()); }
        if self.deprecated { result.push("This node is deprecated".into()); }
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::custom_nodes::*;
    #[test]
    fn consent_and_trust_are_exact_and_namespaced() {
        let a = NodeSource::Registry { id: "a".into(), publisher: Some("p".into()), version: "1".into(), repository: Some("https://host/Repo".into()) };
        let mut b = a.clone();
        if let NodeSource::Registry { id, .. } = &mut b { *id = "b".into(); }
        assert_ne!(a.trust_namespace(), b.trust_namespace());
        let c = InstallConsent { plan_id: "one".into(), binding: "digest".into(), execute_node_code: true, run_install_py: false, allow_source_builds: false, acknowledge_warnings: false, remember_source: false, remember_install_py: false };
        assert!(c.check("two", "digest", false, false).is_err());
        assert!(c.check("one", "changed", false, false).is_err());
        assert!(c.check("one", "digest", true, false).is_err());
        assert!(c.check("one", "digest", false, false).is_ok());
    }
}

#[cfg(test)]
mod classifier_tests {
    use super::*;

    fn metadata(os: &[&str], accelerators: &[&str]) -> CompatibilityMetadata {
        CompatibilityMetadata {
            supported_os: Some(os.iter().map(|s| (*s).into()).collect()),
            supported_accelerators: Some(accelerators.iter().map(|s| (*s).into()).collect()),
            ..Default::default()
        }
    }

    #[test]
    fn upstream_classifiers_are_recognized() {
        assert_eq!(metadata(&["Microsoft :: Windows"], &[]).os_support(HostOs::Windows), Support::Compatible);
        assert_eq!(metadata(&["Microsoft :: Windows"], &[]).os_support(HostOs::Linux), Support::Incompatible);
        assert_eq!(metadata(&["POSIX :: Linux"], &[]).os_support(HostOs::Linux), Support::Compatible);
        assert_eq!(metadata(&["OS Independent"], &[]).os_support(HostOs::Windows), Support::Compatible);
        assert_eq!(metadata(&["Something odd"], &[]).os_support(HostOs::Windows), Support::Unknown);
        assert_eq!(CompatibilityMetadata::default().os_support(HostOs::Linux), Support::Unknown);
        assert_eq!(metadata(&[], &["GPU :: NVIDIA CUDA"]).accelerator_support(true), Support::Compatible);
        assert_eq!(metadata(&[], &["GPU :: NVIDIA CUDA"]).accelerator_support(false), Support::Incompatible);
        assert_eq!(CompatibilityMetadata::default().accelerator_support(true), Support::Unknown);
    }

    #[test]
    fn unknown_values_warn_instead_of_passing_silently() {
        let compat = Compatibility { os: Support::Unknown, accelerator: Support::Compatible, comfyui: Support::Incompatible, flagged: true, deprecated: true };
        let warnings = compat.warnings();
        assert_eq!(warnings.len(), 4);
        assert!(warnings.iter().any(|w| w.contains("OS") && w.contains("unknown")));
        let flagged = CompatibilityMetadata { status: Some("NodeStatusBanned".into()), ..Default::default() };
        assert!(flagged.is_flagged());
        assert!(!CompatibilityMetadata { status: Some("NodeStatusActive".into()), ..Default::default() }.is_flagged());
    }
}

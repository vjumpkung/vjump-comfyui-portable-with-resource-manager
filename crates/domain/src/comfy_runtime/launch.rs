use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LaunchError {
    #[error("the port must be between 1 and 65535")]
    InvalidPort,
    #[error("invalid or reserved extra argument: {0}")]
    InvalidArgument(String),
    #[error("the selected checkout does not support {0}")]
    Unsupported(String),
    #[error("the installed environment has not verified support for {0}")]
    MissingCapability(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Port(u16);
impl Port {
    pub fn new(value: u16) -> Result<Self, LaunchError> {
        if value == 0 {
            Err(LaunchError::InvalidPort)
        } else {
            Ok(Self(value))
        }
    }
    pub fn get(self) -> u16 {
        self.0
    }
}
impl Default for Port {
    fn default() -> Self {
        Self(8188)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VramMode {
    #[default]
    Auto,
    Low,
    High,
    NoVram,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Attention {
    #[default]
    Default,
    Sage,
    Flash,
    Pytorch,
    Split,
    Quad,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreviewMethod {
    #[default]
    Auto,
    Latent2Rgb,
    Taesd,
    None,
}
impl PreviewMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Latent2Rgb => "latent2rgb",
            Self::Taesd => "taesd",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub port: Port,
    pub vram: VramMode,
    pub attention: Attention,
    pub preview: PreviewMethod,
    pub enable_manager: bool,
    pub listen_lan: bool,
    /// Already tokenized argv, never a shell command.
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchCapabilities {
    pub options: BTreeSet<String>,
    pub sage: bool,
    pub flash: bool,
    pub builtin_manager: bool,
    pub legacy_manager: bool,
    /// The environment's torch is a CPU-only build, so ComfyUI must run with `--cpu`.
    pub cpu: bool,
}

fn reserved(name: &str) -> bool {
    const EXACT: &[&str] = &[
        "--listen",
        "--port",
        "--tls-keyfile",
        "--tls-certfile",
        "--auto-launch",
        "--disable-auto-launch",
        "--windows-standalone-build",
        "--lowvram",
        "--highvram",
        "--novram",
        "--normalvram",
        "--gpu-only",
        "--cpu",
        "--use-sage-attention",
        "--use-flash-attention",
        "--use-pytorch-cross-attention",
        "--use-split-cross-attention",
        "--use-quad-cross-attention",
        "--disable-xformers",
        "--preview-method",
        "--enable-manager",
        "--disable-manager-ui",
        "--enable-manager-legacy-ui",
        "--base-directory",
        "--extra-model-paths-config",
        "--output-directory",
        "--input-directory",
        "--user-directory",
        "--temp-directory",
        "--front-end-root",
        "--front-end-version",
        "--help",
        "-h",
    ];
    EXACT.contains(&name)
        || name.contains("directory")
        || name.contains("path")
        || name.contains("manager")
        || name.starts_with("--tls")
}

impl LaunchOptions {
    pub fn listen_address(&self) -> &'static str {
        if self.listen_lan {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
    }

    pub fn validate(&self) -> Result<(), LaunchError> {
        if self.extra_args.len() > 128 {
            return Err(LaunchError::InvalidArgument("too many arguments".into()));
        }
        let mut has_option = false;
        for arg in &self.extra_args {
            if arg.is_empty() || arg.len() > 4096 || arg.chars().any(char::is_control) {
                return Err(LaunchError::InvalidArgument(
                    "empty or invalid token".into(),
                ));
            }
            if arg.starts_with('-') {
                let name = arg.split('=').next().unwrap_or(arg);
                if !name.starts_with("--") || name == "--" || reserved(name) {
                    return Err(LaunchError::InvalidArgument(name.into()));
                }
                has_option = true;
            } else if !has_option {
                return Err(LaunchError::InvalidArgument(arg.clone()));
            }
        }
        Ok(())
    }

    pub fn args(
        &self,
        windows: bool,
        capabilities: &LaunchCapabilities,
    ) -> Result<Vec<String>, LaunchError> {
        self.validate()?;
        if self.attention == Attention::Sage && !capabilities.sage {
            return Err(LaunchError::MissingCapability("SageAttention".into()));
        }
        if self.attention == Attention::Flash && !capabilities.flash {
            return Err(LaunchError::MissingCapability("FlashAttention".into()));
        }
        if self.enable_manager && !capabilities.builtin_manager {
            return Err(LaunchError::MissingCapability(
                "built-in ComfyUI Manager".into(),
            ));
        }
        let mut args = vec![
            "-s".into(),
            "main.py".into(),
            "--listen".into(),
            self.listen_address().into(),
            "--port".into(),
            self.port.get().to_string(),
            "--disable-auto-launch".into(),
            "--preview-method".into(),
            self.preview.as_str().into(),
        ];
        if windows {
            args.push("--windows-standalone-build".into());
        }
        // ComfyUI rejects `--cpu` together with a VRAM flag, so the VRAM mode is ignored on CPU.
        let vram = match self.vram {
            _ if capabilities.cpu => Some("--cpu"),
            VramMode::Auto => None,
            VramMode::Low => Some("--lowvram"),
            VramMode::High => Some("--highvram"),
            VramMode::NoVram => Some("--novram"),
        };
        let attention = match self.attention {
            Attention::Default => None,
            Attention::Sage => Some("--use-sage-attention"),
            Attention::Flash => Some("--use-flash-attention"),
            Attention::Pytorch => Some("--use-pytorch-cross-attention"),
            Attention::Split => Some("--use-split-cross-attention"),
            Attention::Quad => Some("--use-quad-cross-attention"),
        };
        for option in vram.into_iter().chain(attention) {
            args.push(option.into());
        }
        if self.enable_manager {
            args.push("--enable-manager".into());
        }
        for option in args.iter().filter(|arg| arg.starts_with("--")) {
            if !capabilities.options.contains(option) {
                return Err(LaunchError::Unsupported(option.clone()));
            }
        }
        for arg in &self.extra_args {
            if arg.starts_with('-') {
                let name = arg.split('=').next().unwrap_or(arg);
                if !capabilities.options.contains(name) {
                    return Err(LaunchError::Unsupported(name.into()));
                }
            }
        }
        args.extend(self.extra_args.clone());
        Ok(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_overrides_abbreviations_and_path_changes() {
        for token in [
            "--port=9000",
            "--listen",
            "--extra-model-paths-config=x",
            "-p",
            "--tls-keyfile=x",
            "--enable-manager",
            "--base-directory=x",
        ] {
            let options = LaunchOptions {
                extra_args: vec![token.into()],
                ..Default::default()
            };
            assert!(options.validate().is_err(), "{token}");
        }
        let options = LaunchOptions {
            extra_args: vec!["--lis".into()],
            ..Default::default()
        };
        assert!(options.args(false, &LaunchCapabilities::default()).is_err());
        assert!(Port::new(1).is_ok());
        assert!(Port::new(0).is_err());
    }
    #[test]
    fn argv_is_controlled_and_keeps_tokens() {
        let caps = LaunchCapabilities {
            options: [
                "--listen",
                "--port",
                "--disable-auto-launch",
                "--preview-method",
                "--windows-standalone-build",
                "--some-option",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            ..Default::default()
        };
        let options = LaunchOptions {
            extra_args: vec!["--some-option".into(), "a folder ไทย".into()],
            ..Default::default()
        };
        let args = options.args(true, &caps).unwrap();
        assert_eq!(&args[..4], ["-s", "main.py", "--listen", "127.0.0.1"]);
        assert_eq!(args.last().unwrap(), "a folder ไทย");
        assert!(args.contains(&"--disable-auto-launch".into()));
    }
    #[test]
    fn cpu_environments_run_with_cpu_and_never_with_a_vram_flag() {
        let mut caps = LaunchCapabilities {
            options: [
                "--listen",
                "--port",
                "--disable-auto-launch",
                "--preview-method",
                "--windows-standalone-build",
                "--cpu",
                "--lowvram",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            cpu: true,
            ..Default::default()
        };
        let options = LaunchOptions {
            vram: VramMode::Low,
            ..Default::default()
        };
        let args = options.args(true, &caps).unwrap();
        assert!(args.contains(&"--cpu".into()));
        assert!(!args.contains(&"--lowvram".into()));
        caps.options.remove("--cpu");
        assert_eq!(
            options.args(true, &caps),
            Err(LaunchError::Unsupported("--cpu".into()))
        );
        caps.cpu = false;
        let args = options.args(true, &caps).unwrap();
        assert!(args.contains(&"--lowvram".into()));
        assert!(!args.contains(&"--cpu".into()));
    }
}

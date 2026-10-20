//! The on-disk layout of an install root (spec section 3.2) and the controlled environment
//! every subprocess receives.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use domain::environment::EnvId;

use crate::shared::process::{CommandSpec, system_baseline};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn envs_dir(&self) -> PathBuf {
        self.root.join("envs")
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.root.join("tools")
    }

    pub fn uv_dir(&self) -> PathBuf {
        self.tools_dir().join("uv")
    }

    pub fn git_dir(&self) -> PathBuf {
        self.tools_dir().join("git")
    }

    pub fn hf_dir(&self) -> PathBuf {
        self.tools_dir().join("hf")
    }

    pub fn comfyui_dir(&self) -> PathBuf {
        self.root.join("ComfyUI")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir().join("cache")
    }

    pub fn managed_python_dir(&self) -> PathBuf {
        self.cache_dir().join("uv-python")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir().join("logs")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.data_dir().join("downloads")
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("install.json")
    }

    pub fn uv_exe(&self) -> PathBuf {
        exe(&self.uv_dir(), "uv")
    }

    /// The Git executable under `tools/git` (Windows MinGit only).
    pub fn mingit_exe(&self) -> PathBuf {
        self.git_dir().join("cmd").join("git.exe")
    }

    pub fn hf_exe(&self) -> PathBuf {
        venv_scripts(&self.hf_dir()).join(if cfg!(windows) { "hf.exe" } else { "hf" })
    }

    pub fn constraints_file(&self, env: &EnvId) -> PathBuf {
        self.cache_dir().join(format!("constraints-{env}.txt"))
    }

    pub fn setup_log(&self, run_id: &str) -> PathBuf {
        self.logs_dir().join(format!("setup-{run_id}.log"))
    }
}

pub fn exe(directory: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        directory.join(format!("{name}.exe"))
    } else {
        directory.join(name)
    }
}

/// `Scripts` on Windows, `bin` elsewhere.
pub fn venv_scripts(venv: &Path) -> PathBuf {
    venv.join(if cfg!(windows) { "Scripts" } else { "bin" })
}

pub fn venv_python(venv: &Path) -> PathBuf {
    exe(&venv_scripts(venv), "python")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// uv, with the app-local Git and uv directories first on `PATH`.
    Uv,
    Git,
    /// A Python process of an environment (verification, probes).
    Python,
    /// Anything else that only needs the OS baseline (for example `nvidia-smi`).
    Plain,
}

/// `USERNAME` is required: Python 3.12's `getpass.getuser()` (used by torch) otherwise falls
/// back to the Unix-only `pwd` module and fails. The user name is already part of `USERPROFILE`.
const USER_VARIABLES_WINDOWS: [&str; 8] = [
    "USERNAME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "HOMEDRIVE",
    "HOMEPATH",
    "TEMP",
    "TMP",
];
const USER_VARIABLES_UNIX: [&str; 8] = [
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "LANG",
    "LC_ALL",
];
/// Network configuration the user's own tools rely on. These are never credentials the app
/// adds; the app never places tokens in a child environment (NFR-SEC-2).
const NETWORK_VARIABLES: [&str; 14] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "SSH_AUTH_SOCK",
];

fn parent_vars(names: &[&str]) -> Vec<(OsString, OsString)> {
    names
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(*name), value)))
        .collect()
}

/// Builds the complete, allow-listed environment for a child. Nothing else is inherited, and
/// `GIT_DIR` / `GIT_WORK_TREE` are absent by construction.
pub fn tool_environment(layout: &Layout, tool: Tool) -> Vec<(OsString, OsString)> {
    let mut env = system_baseline();
    env.extend(parent_vars(if cfg!(windows) {
        &USER_VARIABLES_WINDOWS
    } else {
        &USER_VARIABLES_UNIX
    }));
    if matches!(tool, Tool::Uv | Tool::Git) {
        env.extend(parent_vars(&NETWORK_VARIABLES));
    }

    let mut path_entries: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        path_entries.push(layout.git_dir().join("cmd"));
    }
    path_entries.push(layout.uv_dir());
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
        let root = PathBuf::from(root);
        path_entries.push(root.join("System32"));
        path_entries.push(root);
    } else {
        for entry in ["/usr/local/bin", "/usr/bin", "/bin"] {
            path_entries.push(PathBuf::from(entry));
        }
    }
    let joined = std::env::join_paths(&path_entries).unwrap_or_default();
    env.push(("PATH".into(), joined));

    let cache = layout.cache_dir();
    let path_var = |name: &str, path: PathBuf| (OsString::from(name), path.into_os_string());
    env.push(path_var("UV_CACHE_DIR", cache.join("uv")));
    env.push(path_var(
        "UV_PYTHON_INSTALL_DIR",
        layout.managed_python_dir(),
    ));
    env.push(path_var("PIP_CACHE_DIR", cache.join("pip")));
    env.push(path_var("HF_HOME", cache.join("hf")));
    env.push(path_var("TORCH_HOME", cache.join("torch")));
    env.push(path_var("TRITON_HOME", cache.join("triton")));
    for (name, value) in [
        ("PYTHONNOUSERSITE", "1"),
        ("PYTHONUTF8", "1"),
        ("PYTHONDONTWRITEBYTECODE", "1"),
        ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("NO_COLOR", "1"),
        ("UV_NO_PROGRESS", "1"),
        ("UV_HTTP_TIMEOUT", "120"),
        // Explicit `uv python install` works; implicit downloads never happen.
        ("UV_PYTHON_DOWNLOADS", "manual"),
    ] {
        env.push((name.into(), value.into()));
    }
    env
}

/// Ensures the cache directories exist so tools never fall back to a default location.
pub fn ensure_cache_dirs(layout: &Layout) -> std::io::Result<()> {
    for name in ["uv", "uv-python", "pip", "hf", "torch", "triton"] {
        std::fs::create_dir_all(layout.cache_dir().join(name))?;
    }
    std::fs::create_dir_all(layout.downloads_dir())?;
    std::fs::create_dir_all(layout.logs_dir())
}

/// A command with the controlled environment of `tool`.
pub fn command(layout: &Layout, tool: Tool, program: impl Into<PathBuf>) -> CommandSpec {
    CommandSpec::new(program).env_vars(tool_environment(layout, tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(env: &[(OsString, OsString)]) -> Vec<String> {
        env.iter()
            .map(|(name, _)| name.to_string_lossy().to_uppercase())
            .collect()
    }

    fn value(env: &[(OsString, OsString)], name: &str) -> Option<String> {
        env.iter()
            .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    }

    #[test]
    fn environment_is_an_allow_list_with_isolated_caches() {
        let root = PathBuf::from("/install root ไทย");
        let layout = Layout::new(&root);
        let env = tool_environment(&layout, Tool::Uv);
        let names = names(&env);
        for isolated in [
            "UV_CACHE_DIR",
            "UV_PYTHON_INSTALL_DIR",
            "PIP_CACHE_DIR",
            "HF_HOME",
            "TORCH_HOME",
            "TRITON_HOME",
            "PYTHONNOUSERSITE",
            "PYTHONUTF8",
        ] {
            assert!(names.contains(&isolated.to_owned()), "{isolated}");
        }
        for forbidden in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "HF_TOKEN",
            "HUGGING_FACE_HUB_TOKEN",
            "CIVITAI_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "PIP_INDEX_URL",
            "UV_INDEX_URL",
            "PYTHONPATH",
            "VIRTUAL_ENV",
        ] {
            assert!(!names.contains(&forbidden.to_owned()), "{forbidden}");
        }
        assert_eq!(value(&env, "PYTHONNOUSERSITE").as_deref(), Some("1"));
        assert_eq!(
            value(&env, "UV_PYTHON_DOWNLOADS").as_deref(),
            Some("manual")
        );
        assert!(
            value(&env, "UV_CACHE_DIR")
                .unwrap()
                .contains(&root.to_string_lossy().into_owned())
        );
        let path = value(&env, "PATH").unwrap();
        assert!(path.contains("tools"), "tool directories lead PATH: {path}");
    }

    #[test]
    fn arbitrary_parent_variables_are_not_inherited() {
        // Every name in the result is either an isolation variable, an OS baseline variable or
        // an explicit user/network allow-list entry.
        let layout = Layout::new("/install");
        let allowed: Vec<String> = system_baseline()
            .iter()
            .map(|(name, _)| name.to_string_lossy().to_uppercase())
            .chain(
                USER_VARIABLES_WINDOWS
                    .iter()
                    .chain(USER_VARIABLES_UNIX.iter())
                    .chain(NETWORK_VARIABLES.iter())
                    .map(|name| name.to_uppercase()),
            )
            .chain(
                [
                    "PATH",
                    "UV_CACHE_DIR",
                    "UV_PYTHON_INSTALL_DIR",
                    "PIP_CACHE_DIR",
                    "HF_HOME",
                    "TORCH_HOME",
                    "TRITON_HOME",
                    "PYTHONNOUSERSITE",
                    "PYTHONUTF8",
                    "PYTHONDONTWRITEBYTECODE",
                    "PIP_DISABLE_PIP_VERSION_CHECK",
                    "GIT_TERMINAL_PROMPT",
                    "NO_COLOR",
                    "UV_NO_PROGRESS",
                    "UV_HTTP_TIMEOUT",
                    "UV_PYTHON_DOWNLOADS",
                ]
                .map(str::to_owned),
            )
            .collect();
        for tool in [Tool::Uv, Tool::Git, Tool::Python, Tool::Plain] {
            for name in names(&tool_environment(&layout, tool)) {
                assert!(allowed.contains(&name), "{name} is not on the allow-list");
            }
        }
        // Proxy configuration reaches only the tools that fetch from the network.
        let python = names(&tool_environment(&layout, Tool::Python));
        assert!(!python.contains(&"HTTPS_PROXY".to_owned()));
    }

    #[cfg(windows)]
    #[test]
    fn python_children_get_the_windows_user_name() {
        let Some(user_name) = std::env::var_os("USERNAME") else {
            return;
        };
        let layout = Layout::new("/install");
        let env = tool_environment(&layout, Tool::Python);
        assert_eq!(
            value(&env, "USERNAME"),
            Some(user_name.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn paths_follow_the_documented_layout() {
        let layout = Layout::new("/r");
        assert_eq!(layout.envs_dir(), PathBuf::from("/r/envs"));
        assert_eq!(layout.comfyui_dir(), PathBuf::from("/r/ComfyUI"));
        assert_eq!(layout.manifest_path(), PathBuf::from("/r/install.json"));
        assert_eq!(
            layout.setup_log("abc"),
            PathBuf::from("/r/data/logs/setup-abc.log")
        );
        let env = EnvId::new("env-1").unwrap();
        assert_eq!(
            layout.constraints_file(&env),
            PathBuf::from("/r/data/cache/constraints-env-1.txt")
        );
        assert!(venv_python(Path::new("/v")).ends_with(if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        }));
    }
}

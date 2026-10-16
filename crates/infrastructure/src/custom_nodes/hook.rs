//! Runs a node's root `install.py` with the environment's interpreter. Only called after the
//! explicit consent recorded in the plan; a virtual environment is not a sandbox.
use super::tree::is_link;
use crate::{
    environment::{
        exec::run_step,
        layout::{Layout, Tool, command, venv_python},
    },
    shared::process::RunLimits,
};
use application::{
    custom_nodes::{NodeAppError, NodeHook},
    environment::{ProvisionError, StepContext},
};
use async_trait::async_trait;
use domain::environment::PythonEnvironment;
use std::{path::Path, time::Duration};

pub struct PythonHook;

#[async_trait]
impl NodeHook for PythonHook {
    async fn run(
        &self,
        root: &Path,
        env: &PythonEnvironment,
        node_directory: &Path,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        let layout = Layout::new(root);
        let custom_nodes = layout.comfyui_dir().join("custom_nodes");
        let script = node_directory.join("install.py");
        let inside = std::fs::canonicalize(node_directory)
            .ok()
            .zip(std::fs::canonicalize(&custom_nodes).ok())
            .is_some_and(|(node, base)| node.starts_with(base));
        let regular = script
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_file() && !is_link(&meta));
        if !inside || !regular {
            return Err(NodeAppError::UnsafeSource);
        }
        // `-s` disables the user site; the child receives the controlled allow-listed environment.
        let spec = command(&layout, Tool::Python, venv_python(&env.path))
            .args(["-s", "install.py"])
            .cwd(node_directory);
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(30 * 60)),
            ..Default::default()
        };
        match run_step(&spec, ctx, &limits).await {
            Ok(_) => Ok(()),
            Err(ProvisionError::Cancelled) => Err(NodeAppError::Cancelled),
            Err(_) => Err(NodeAppError::DependenciesFailed),
        }
    }
}

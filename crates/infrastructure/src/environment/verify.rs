//! Verification of an environment by running small Python snippets in it.

use std::{path::Path, time::Duration};

use application::environment::{
    EnvVerifier, ProvisionError, StepContext, TorchReport, VerificationReport,
};
use async_trait::async_trait;
use domain::environment::VerifyExpectations;
use serde::Deserialize;

use super::{
    exec::run_step,
    layout::{Layout, Tool, command, venv_python},
};
use crate::shared::process::RunLimits;

const MARKER: &str = "VJ-VERIFY:";

const TORCH_SCRIPT: &str = r#"
import json, torch
print("VJ-VERIFY:" + json.dumps({"version": torch.__version__, "cuda": bool(torch.cuda.is_available())}))
"#;

const TRITON_SCRIPT: &str = r#"
import json, triton
print("VJ-VERIFY:" + json.dumps({"version": triton.__version__}))
"#;

/// The reference verification (`import sys, torch, triton, comfy`), plus a real CUDA tensor
/// operation when CUDA is available, and the presence of the optional accelerator modules.
const FINAL_SCRIPT: &str = r#"
import importlib.util, json, sys
import torch, triton, comfy
info = {
    "python": sys.version.split()[0],
    "torch": torch.__version__,
    "cuda": bool(torch.cuda.is_available()),
    "triton": triton.__version__,
    "comfy": list(comfy.__path__)[0],
    "device": None,
    "cuda_ok": None,
}
if info["cuda"]:
    value = torch.ones(8, device="cuda").add(1).sum().item()
    torch.cuda.synchronize()
    info["cuda_ok"] = value == 16.0
    info["device"] = torch.cuda.get_device_name(0)
info["accelerators"] = {
    name: importlib.util.find_spec(name) is not None
    for name in ("sageattention", "flash_attn", "llama_cpp")
}
print("VJ-VERIFY:" + json.dumps(info))
"#;

#[derive(Deserialize)]
struct TorchInfo {
    version: String,
    cuda: bool,
}

#[derive(Deserialize)]
struct TritonInfo {
    version: String,
}

#[derive(Deserialize)]
struct FinalInfo {
    python: String,
    torch: String,
    cuda: bool,
    triton: String,
    comfy: String,
    device: Option<String>,
    cuda_ok: Option<bool>,
    accelerators: std::collections::BTreeMap<String, bool>,
}

fn marker_json(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix(MARKER))
}

fn parse<T: for<'de> Deserialize<'de>>(stdout: &str) -> Result<T, ProvisionError> {
    marker_json(stdout)
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or_else(|| ProvisionError::Failed("the verification produced no result".into()))
}

/// Checks the torch report against the profile's expectations.
pub fn check_torch(
    report: &TorchReport,
    expectations: &VerifyExpectations,
) -> Result<(), ProvisionError> {
    if !report
        .version
        .starts_with(&expectations.torch_version_prefix)
    {
        return Err(ProvisionError::Failed(format!(
            "PyTorch reports version {} but the profile expects {}.",
            report.version, expectations.torch_version_prefix
        )));
    }
    if expectations.require_cuda && !report.cuda_available {
        return Err(ProvisionError::Failed(
            "PyTorch cannot use CUDA. Check the NVIDIA driver and that the GPU is supported by this profile."
                .into(),
        ));
    }
    Ok(())
}

pub struct PythonVerifier;

impl PythonVerifier {
    pub fn new() -> Self {
        Self
    }

    async fn run(
        &self,
        root: &Path,
        env_path: &Path,
        script: &str,
        cwd: Option<&Path>,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        let layout = Layout::new(root);
        let mut spec =
            command(&layout, Tool::Python, venv_python(env_path)).args(["-s", "-c", script]);
        if let Some(cwd) = cwd {
            spec = spec.cwd(cwd);
        }
        let limits = RunLimits {
            timeout: Some(Duration::from_secs(600)),
            ..RunLimits::default()
        };
        run_step(&spec, ctx, &limits)
            .await
            .map(|output| output.stdout)
    }
}

impl Default for PythonVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EnvVerifier for PythonVerifier {
    async fn verify_torch(
        &self,
        root: &Path,
        env_path: &Path,
        expectations: &VerifyExpectations,
        ctx: &StepContext,
    ) -> Result<TorchReport, ProvisionError> {
        let stdout = self.run(root, env_path, TORCH_SCRIPT, None, ctx).await?;
        let info: TorchInfo = parse(&stdout)?;
        let report = TorchReport {
            version: info.version,
            cuda_available: info.cuda,
        };
        check_torch(&report, expectations)?;
        Ok(report)
    }

    async fn verify_triton(
        &self,
        root: &Path,
        env_path: &Path,
        ctx: &StepContext,
    ) -> Result<String, ProvisionError> {
        let stdout = self.run(root, env_path, TRITON_SCRIPT, None, ctx).await?;
        Ok(parse::<TritonInfo>(&stdout)?.version)
    }

    async fn verify_final(
        &self,
        root: &Path,
        env_path: &Path,
        expectations: &VerifyExpectations,
        ctx: &StepContext,
    ) -> Result<VerificationReport, ProvisionError> {
        let layout = Layout::new(root);
        let comfy_dir = layout.comfyui_dir();
        let stdout = self
            .run(root, env_path, FINAL_SCRIPT, Some(&comfy_dir), ctx)
            .await?;
        let info: FinalInfo = parse(&stdout)?;
        check_torch(
            &TorchReport {
                version: info.torch.clone(),
                cuda_available: info.cuda,
            },
            expectations,
        )?;
        if expectations.require_cuda && info.cuda_ok != Some(true) {
            return Err(ProvisionError::Failed(
                "A CUDA tensor operation did not produce the expected result.".into(),
            ));
        }
        Ok(VerificationReport {
            python_version: info.python,
            torch_version: info.torch,
            cuda_available: info.cuda,
            cuda_device: info.device,
            triton_version: Some(info.triton),
            comfy_path: Some(info.comfy),
            accelerators: info.accelerators.into_iter().collect(),
        })
    }
}

#[cfg(test)]
mod tests;

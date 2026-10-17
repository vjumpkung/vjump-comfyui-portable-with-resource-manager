//! Runs a node's guarded root `install.py` with the environment's interpreter. Only called after
//! the explicit consent recorded in the plan; a virtual environment is not a sandbox.
use crate::{
    environment::{
        exec::run_step,
        layout::{Layout, Tool, command, venv_python},
    },
    shared::process::RunLimits,
};
use application::{
    custom_nodes::{NodeAppError, NodeHook, NodeLocationGuard},
    environment::{ProvisionError, StepContext},
};
use async_trait::async_trait;
use domain::environment::PythonEnvironment;
use std::time::Duration;

/// Runs the script text received on standard input as `__main__` of `install.py` in the working
/// directory. `__file__`, `sys.argv[0]`, `sys.path[0]`, the working directory and the import
/// context match `python -s install.py`, but the bytes are the guarded snapshot: the file is
/// never reopened by name between review and execution.
const BOOT: &str = "\
import __main__, os, sys
path = os.path.join(os.getcwd(), 'install.py')
source = sys.stdin.buffer.read()
sys.argv[:] = ['install.py']
sys.path[0] = os.path.dirname(path)
__main__.__dict__['__file__'] = path
exec(compile(source, path, 'exec', dont_inherit=True), __main__.__dict__)
";

pub struct PythonHook;

#[async_trait]
impl NodeHook for PythonHook {
    async fn run(
        &self,
        root: &std::path::Path,
        env: &PythonEnvironment,
        location: &dyn NodeLocationGuard,
        ctx: &StepContext,
    ) -> Result<(), NodeAppError> {
        let layout = Layout::new(root);
        let script = location
            .install_script()
            .ok_or(NodeAppError::UnsafeSource)?;
        // `-s` disables the user site; the child receives the controlled allow-listed environment.
        // The guard stays retained by the command until every process of the script has exited.
        let spec = command(&layout, Tool::Python, venv_python(&env.path))
            .args(["-s", "-c", BOOT])
            .cwd(location.directory())
            .stdin(script)
            .retain(location.retained());
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

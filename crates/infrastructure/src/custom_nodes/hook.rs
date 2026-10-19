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
pub(crate) const BOOT: &str = r#"
def _vjump_boot():
    import sys
    initial_empty = bool(sys.path and sys.path[0] == '')
    if initial_empty:
        del sys.path[0]
    import hashlib, os, __main__
    from importlib.machinery import SourceFileLoader
    length = int(sys.argv[1])
    expected = sys.argv[2]
    source = sys.stdin.buffer.read(length + 1)
    if len(source) != length or hashlib.sha256(source).hexdigest() != expected:
        raise SystemExit('The guarded command input could not be verified.')
    path = os.path.abspath('install.py')
    code = compile(source, path, 'exec', dont_inherit=True)
    sys.argv[:] = ['install.py']
    if initial_empty:
        sys.path.insert(0, os.path.dirname(path))
    namespace = __main__.__dict__
    namespace.update(__file__=path, __package__=None, __spec__=None, __cached__=None,
                     __loader__=SourceFileLoader('__main__', path))
    del namespace['_vjump_boot']
    exec(code, namespace)
_vjump_boot()
"#;

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
            .arg(script.len().to_string())
            .arg(crate::shared::download::sha256_hex(&script))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::process::{self, CommandSpec, DiscardSink, RunError};
    use application::shared::CancelToken;
    use std::{path::Path, sync::Arc};

    fn python() -> std::path::PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
            .map(|dir| {
                dir.join(if cfg!(windows) {
                    "python.exe"
                } else {
                    "python"
                })
            })
            .find(|path| path.is_file())
            .expect("real Python is required")
    }

    async fn boot(
        directory: &Path,
        expected: &[u8],
        delivered: &[u8],
        safe: bool,
    ) -> Result<process::Output, RunError> {
        let mut env = process::system_baseline();
        if safe {
            env.push(("PYTHONSAFEPATH".into(), "1".into()));
        }
        let spec = CommandSpec::new(python())
            .args(["-s", "-c", BOOT])
            .arg(expected.len().to_string())
            .arg(crate::shared::download::sha256_hex(expected))
            .cwd(directory)
            .stdin(Arc::from(delivered))
            .env_vars(env);
        process::run(
            &spec,
            &DiscardSink,
            &CancelToken::new(),
            &RunLimits {
                timeout: Some(Duration::from_secs(20)),
                ..Default::default()
            },
        )
        .await
    }

    #[tokio::test]
    async fn guarded_hook_rejects_truncated_or_changed_input_before_exec() {
        let directory = tempfile::tempdir().unwrap();
        let script = b"open('executed', 'w').write('yes')\n# guarded\n";
        let mut appended = script.to_vec();
        appended.push(b' ');
        let mut changed = script.to_vec();
        *changed.last_mut().unwrap() = b' ';
        // Hashing must not import this node-controlled module while verifying input.
        std::fs::write(
            directory.path().join("hashlib.py"),
            "open('hash-intercepted', 'w').write('yes')\n",
        )
        .unwrap();
        for bytes in [
            &script[..script.len() - 1],
            appended.as_slice(),
            changed.as_slice(),
        ] {
            assert!(boot(directory.path(), script, bytes, false).await.is_err());
            assert!(!directory.path().join("executed").exists());
            assert!(!directory.path().join("hash-intercepted").exists());
        }
        boot(directory.path(), script, script, false).await.unwrap();
        assert!(directory.path().join("executed").exists());
        assert!(!directory.path().join("hash-intercepted").exists());
    }

    #[tokio::test]
    async fn guarded_hook_has_a_clean_main_namespace() {
        let directory = tempfile::tempdir().unwrap();
        let script = br#"assert not {'_vjump_boot', 'path', 'source', 'namespace', 'hashlib', 'sys', 'os', '__main__', 'length', 'expected', 'initial_empty', 'SourceFileLoader', 'code'} & globals().keys()
import sys, os, importlib.machinery
assert __name__ == '__main__'
assert __package__ is None and __spec__ is None and __cached__ is None
assert __file__ == os.path.abspath('install.py')
assert sys.argv == ['install.py']
assert isinstance(__loader__, importlib.machinery.SourceFileLoader)
assert __loader__.name == '__main__' and __loader__.path == __file__
open('verified', 'w').write('yes')
"#;
        boot(directory.path(), script, script, false).await.unwrap();
        assert!(directory.path().join("verified").exists());
        // compile(bytes) must retain Python's encoding-cookie and BOM handling.
        for script in [
            b"# coding: latin-1\nx = '\xe9'\n".as_slice(),
            b"\xef\xbb\xbfx = 'bom'\n".as_slice(),
        ] {
            boot(directory.path(), script, script, false).await.unwrap();
        }
    }

    #[tokio::test]
    async fn guarded_hook_respects_safe_path_mode() {
        let directory = tempfile::tempdir().unwrap();
        let script = b"import sys, os\nassert sys.flags.safe_path\nassert '' not in sys.path\nassert os.getcwd() not in sys.path\nopen('safe', 'w').write('yes')\n";
        boot(directory.path(), script, script, true).await.unwrap();
        assert!(directory.path().join("safe").exists());
    }
}

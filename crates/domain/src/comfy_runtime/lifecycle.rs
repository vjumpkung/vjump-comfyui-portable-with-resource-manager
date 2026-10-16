use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lifecycle {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Spawn,
    ExitBeforeReady,
    ReadinessTimeout,
    UnexpectedExit,
    Termination,
    Cancelled,
    Persistence,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFailure {
    pub kind: FailureKind,
    pub exit_code: Option<i32>,
    pub message: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid runtime lifecycle transition")]
pub struct LifecycleError;
impl Lifecycle {
    pub fn transition(self, to: Self) -> Result<Self, LifecycleError> {
        use Lifecycle::*;
        if matches!(
            (self, to),
            (Stopped | Crashed, Starting)
                | (Starting, Running | Stopping | Crashed)
                | (Running, Stopping | Crashed)
                | (Stopping, Stopped | Crashed)
        ) {
            Ok(to)
        } else {
            Err(LifecycleError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_policy() {
        assert!(Lifecycle::Stopped.transition(Lifecycle::Running).is_err());
        assert_eq!(
            Lifecycle::Starting.transition(Lifecycle::Stopping),
            Ok(Lifecycle::Stopping)
        );
        assert_eq!(
            Lifecycle::Crashed.transition(Lifecycle::Starting),
            Ok(Lifecycle::Starting)
        );
    }
}

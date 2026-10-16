//! Close-request policy. The desktop shell performs the window and exit actions.
use domain::{comfy_runtime::Lifecycle, settings::CloseBehavior};

/// The user's answer to an `Ask` close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    StopAndExit,
    KeepRunning,
    Cancel,
}

/// What the shell must do for a manager close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Nothing is running: exit.
    Exit,
    /// Stop ComfyUI (verified), then exit.
    StopThenExit,
    /// Close the windows but retain the core, supervisor and log readers.
    KeepRunningHeadless,
    /// Ask the user.
    Ask,
}

pub fn close_action(behavior: CloseBehavior, lifecycle: Lifecycle) -> CloseAction {
    if matches!(lifecycle, Lifecycle::Stopped | Lifecycle::Crashed) {
        return CloseAction::Exit;
    }
    match behavior {
        CloseBehavior::Ask => CloseAction::Ask,
        CloseBehavior::StopComfy => CloseAction::StopThenExit,
        CloseBehavior::KeepRunningHeadless => CloseAction::KeepRunningHeadless,
    }
}

pub fn choice_action(choice: CloseChoice) -> Option<CloseAction> {
    match choice {
        CloseChoice::StopAndExit => Some(CloseAction::StopThenExit),
        CloseChoice::KeepRunning => Some(CloseAction::KeepRunningHeadless),
        CloseChoice::Cancel => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_runtime_exits_and_active_runtime_follows_the_setting() {
        for lifecycle in [Lifecycle::Stopped, Lifecycle::Crashed] {
            assert_eq!(
                close_action(CloseBehavior::Ask, lifecycle),
                CloseAction::Exit
            );
        }
        assert_eq!(
            close_action(CloseBehavior::Ask, Lifecycle::Running),
            CloseAction::Ask
        );
        assert_eq!(
            close_action(CloseBehavior::StopComfy, Lifecycle::Starting),
            CloseAction::StopThenExit
        );
        assert_eq!(
            close_action(CloseBehavior::KeepRunningHeadless, Lifecycle::Running),
            CloseAction::KeepRunningHeadless
        );
        assert_eq!(choice_action(CloseChoice::Cancel), None);
    }
}

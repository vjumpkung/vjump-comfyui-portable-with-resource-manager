use std::sync::Mutex;

use crate::AppState;

enum Phase {
    Initializing,
    Ready,
    Exiting,
}

pub(crate) struct StartupGate {
    phase: Mutex<Phase>,
}

impl Default for StartupGate {
    fn default() -> Self {
        Self {
            phase: Mutex::new(Phase::Initializing),
        }
    }
}

impl StartupGate {
    #[allow(clippy::result_large_err)]
    pub(crate) fn publish(
        &self,
        state: AppState,
        publish: impl FnOnce(AppState),
    ) -> Result<(), AppState> {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        if !matches!(*phase, Phase::Initializing) {
            return Err(state);
        }
        publish(state);
        *phase = Phase::Ready;
        Ok(())
    }

    pub(crate) fn begin_exit(&self) -> bool {
        let mut phase = self.phase.lock().unwrap_or_else(|error| error.into_inner());
        match *phase {
            Phase::Initializing | Phase::Exiting => {
                *phase = Phase::Exiting;
                true
            }
            Phase::Ready => false,
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        matches!(
            *self.phase.lock().unwrap_or_else(|error| error.into_inner()),
            Phase::Ready
        )
    }

    pub(crate) fn is_exiting(&self) -> bool {
        matches!(
            *self.phase.lock().unwrap_or_else(|error| error.into_inner()),
            Phase::Exiting
        )
    }
}

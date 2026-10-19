use std::sync::Arc;

/// Opaque protection (pinned directories, open files) that the adapter running a command must
/// keep alive until the command and every process it left behind have exited, even when the
/// caller drops its own guard because the command failed.
#[derive(Clone, Default)]
pub struct Retained(Option<Arc<dyn Send + Sync>>);

impl Retained {
    pub fn new(resources: Arc<dyn Send + Sync>) -> Self {
        Self(Some(resources))
    }

    pub fn merge(self, other: Self) -> Self {
        match (self.0, other.0) {
            (None, other) => Self(other),
            (this, None) => Self(this),
            (Some(this), Some(other)) => Self::new(Arc::new((this, other))),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
}

impl std::fmt::Debug for Retained {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Retained(..)"
        } else {
            "Retained(none)"
        })
    }
}

impl PartialEq for Retained {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Retained {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Owner(Arc<AtomicUsize>);

    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn merge_keeps_every_owner_until_the_last_copy_is_dropped() {
        let released = Arc::new(AtomicUsize::new(0));
        let first = Retained::new(Arc::new(Owner(released.clone())));
        let second = Retained::new(Arc::new(Owner(released.clone())));
        let merged = first
            .clone()
            .merge(second.clone())
            .merge(Retained::default());
        assert!(!merged.is_empty());
        assert!(Retained::default().merge(Retained::default()).is_empty());
        drop((first, second));
        assert_eq!(released.load(Ordering::SeqCst), 0);
        let copy = merged.clone();
        drop(merged);
        assert_eq!(released.load(Ordering::SeqCst), 0);
        drop(copy);
        assert_eq!(released.load(Ordering::SeqCst), 2);
    }
}

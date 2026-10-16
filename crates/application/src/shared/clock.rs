/// Supplies timestamps so workflows stay deterministic in tests.
pub trait Clock: Send + Sync {
    /// The current UTC time formatted as RFC 3339 (`2026-10-16T01:02:03Z`).
    fn now(&self) -> String;
}

/// Supplies unique identifiers for runs, environments and plans.
pub trait IdGenerator: Send + Sync {
    /// A new lowercase hexadecimal identifier.
    fn new_id(&self) -> String;
}

-- Acceptance is recorded in the same transaction as the component-aware path rebase.
CREATE TABLE install_relocations (
    old_root TEXT NOT NULL,
    new_root TEXT NOT NULL,
    accepted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (old_root, new_root)
);

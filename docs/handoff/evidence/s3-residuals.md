# Session 3 — accumulated residuals, gaps and limitations (input for package R / docs)

Each item: source package, statement, evidence, status.

| # | Package | Residual / gap | Evidence | Status |
|---|---|---|---|---|
| 1 | P | After a failed self-update install, downloads stay stopped until the app restarts (the download supervisor has no restart API); the model library is restored. | merge-P coder report; `crates/infrastructure/src/models/acquisition/supervisor.rs` | Known limitation |
| 2 | P | Updater inert without build-time `VJUMP_UPDATER_PUBKEY` + `VJUMP_UPDATER_ENDPOINT`; signing key custody = owner. The file tracing worker stays stopped after the pre-install flush if `install()` returns an error (error appended synchronously to app.log). | `crates/desktop/src/updates.rs` | By design / limitation |
| 3 | B | HF relay on 127.0.0.1 has no per-download authentication: while an HF attempt runs, another local process/user can tunnel to allow-listed HF hosts via the user's upstream proxy. | thinker F16; advisor H plan §6 | Accepted Release-1 residual |
| 4 | H1/WP1 | Linux-only code (openat pins, `/proc/<pid>/fd` anchors, `renameat2`, `O_NONBLOCK`, FIFO test, trash placement) never compiled or run. Post-`renameat2` identity check deferred to a native Linux package. | advisor H plan §6 | Open (needs native Linux) |
| 5 | H1 | Windows containment re-resolves DOS paths inside pinned parents; same-user drive-letter remapping (`subst`, `DefineDosDevice`, `net use`) is outside the boundary. All reparse points are refused, so OneDrive placeholders / dedup / WOF reparse-backed entries are unsupported in node trees. | thinker F14 + false-refusal note; advisor reparse policy | Documented boundary |
| 6 | H2 | `SetDllDirectoryW("")` is NOT inherited by child processes (measured: child still loads a cwd DLL, error 193). The app/runner process excludes cwd; uv/git/python children started with a node directory as cwd keep the default DLL search order (cwd after system dirs under SafeDllSearchMode). | H2 coder report; `crates/infrastructure/src/shared/process/windows.rs` | Residual — re-check with advisor |
| 7 | H2 | A child that exits non-zero before consuming all stdin is reported as `InputFailed` (fixed message) rather than its own exit code. | H2 coder report | Minor limitation |
| 8 | H2 | Git config preflight runs as a separate process before each Git command: a same-user edit of `.git/config` in between is a residual window. | H2 coder report; spec accepts | Accepted |
| 9 | H2 | Pre-consent editable/source refusal: uv 0.13.1 with `--only-binary :all:` already refuses `-e` sources itself; our preflight is proven to run first by its refusal text. | H2 coder report | Evidence note |

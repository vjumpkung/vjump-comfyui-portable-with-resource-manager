# Session 4 — review records (2026-10-19)

The Codex advisor was rate-limited for parts of this session, so the E2 and E3 reviews were done by the
thinker role (Opus, read-only) standing in for the advisor. Advisor outputs that did complete are in
`s4-advisor-first-run-fix.md` and `s4-advisor-wp5-gaps.md`. All corrections listed here were implemented
and verified (see HANDOFF.md §0a).

## E2 — curated setup nodes (thinker): GO with corrections

Accepted deviations:

1. `SetupNodes` returns `SetupNodeError` (distinguishes skippable preparation failures, published dependency damage,
   pin drift, stale plans, missing consent); added `pending(run)` and `installed(run, entry, lease)`.
2. Curated nodes install **detached** at the exact catalog commit (M5 has no "branch at pinned commit"); the catalog
   `branch` is informational; each review carries a detached warning. A later Update needs an explicit reference.
3. install.py never runs in setup (`has_install_py`/`trusted_install_py` forced false, consent flags cleared).
4. `discard_obsolete_setup_plans` (not in spec) is safe: lease held, owned staging only, via `discard_plan`.
5. Retry decisions are process-local (`ActiveRuns`), never persisted.
6. Consent pause surfaces as a `consent_required` step failure without payload; the UI keys on
   `failure.step === 'curated_nodes'` + `curatedReviews`.

Corrections (implemented in `e2b`): persist `CuratedStatus::Failed` on Damaged/PinDrift, never re-prepare a published
entry, `skip()` refuses while any entry is Failed, `verify_curated` covers Failed; `publication_started` fails closed;
install.py guidance = uninstall + reinstall with install.py approval (main's decision, no new signal), note persisted per
entry.

Notes for package I: batch acknowledgement must list the detached warning explicitly; decisions must be re-sent on every
Retry; after a backend restart the run shows `consent_required` with empty `curatedReviews` → offer Retry to re-review;
use `failure.canSkip`, never infer Skip from the step policy; show the informational branch next to the commit.

Residual risks: per-step skip (one unreachable repo or a pre-existing folder blocks the batch); final-validation mismatch
leaves a node op in DependenciesInProgress until a reconcile/restart; executor's cached env copy can overwrite the
candidate record (harmless today); up to 22 clones staged before the single review; no real-Git curated test.

## E3 — missing-root recovery and guided rebuild (thinker): GO with corrections

Main invariant holds: no E3 path writes to the recorded old root (run-root, publish and execute guards; component-aware
rebasing; process scan of both roots before acceptance).

Corrections (implemented in `e3b`):

- C1 (HIGH) restrict composition only when the root moved (roots-only comparison, no interpreter probe at compose); a
  broken interpreter keeps normal composition (`RebuildRequired(InterpreterBroken)` phase only).
- C2 (HIGH) restricted mode runs `ReconcileStartup` under the gate for current-root runs, then re-checks.
- C3 status precedence: a current-root active run is `RunInProgress` before `RebuildRequired`.
- C4 chained relocation rebases from every recorded root (longest component prefix).
- C5 no verbatim `\\?\` paths stored; canonical paths only prove containment.
- C6 logging starts before compose (never for a missing root).
- C7 downloads/library quiesce only in `request_restart` (after success).
- C8 `open_existing` journal refusal only for relocated candidates.
- C9 old-root whole-tree (incl. `data/app.db` bytes) identity asserted.
- C10 recovery compose tolerates a missing bootstrap path (opener unset → `root_switch_failed`).
- Decisions D1–D10: restricted only for Moved; Repair/ChangePython/SwitchVenv/ChangeTorch allowed with a broken
  interpreter; same-root relocation check/accept are no-ops; manifest-valid/DB-missing falls back to root comparison;
  unreadable external paths left unchanged; `.cloning` markers refuse relocation; destinations nested in/around the
  current or old root refused; Rebuild on a healthy install allowed.

Open points reported by the e3b coder: D8 also refuses the recorded old root itself, so a user in the moved restricted
state cannot "Locate" the original install through the UI (revisit in I/R); a moved root with a valid manifest but no
`data/` still ends in the startup-failed recovery; C10 and D2 have no dedicated tests.

Residual risks: move detection needs a valid `install.json`; user-owned paths into the old root are kept; exact Python
patch for an adopted interpreter must be installable by uv; `open_existing` refuses roots under junctions/OneDrive
placeholders; read-only WAL open may create `-wal`/`-shm`; Manager requirements during rebuild come only from the
custom_nodes scan; `AppHandle::restart` skips `RunEvent::Exit` (quiesce happens before it); Linux paths uncompiled;
scenario 4 needs a manual NVIDIA run.

## Flaky runtime tests (Sonnet coder, verified by main)

Root causes: (1) `runtime_guard::assess` treats any unreadable python process on the machine as Unknown and the runtime
tests used the real machine-wide snapshot while other tests spawned interpreters; (2) the fixture dropped its reserved
port before the supervisor probed it. Fix: injectable `procscan::SnapshotFn` (production = real snapshot; tests use
`test_snapshot()`), unique reserved test ports, 700 ms readiness only where the timeout is asserted, bounded spawn waits.
Production behaviour unchanged.

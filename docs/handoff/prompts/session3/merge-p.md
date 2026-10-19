ROLE: coder (write-capable). Mechanically integrate an already-verified package into the main working copy. Do not redesign or expand scope. If underspecified or conflicting with the code, stop that item and report the gap.

GOAL: integrate M6 package P (signed updater + redacted diagnostics) into main and close its three plan gaps.

STATE: main applied `docs/handoff/patches/p-updater-diagnostics-verified.patch` (verified on an older baseline, commit 0ffa136) with `git apply --3way`. Everything applied cleanly except `crates/desktop/src/state.rs`, which contains conflict markers (`<<<<<<< ours` = current main, `>>>>>>> theirs` = P). Since 0ffa136, main gained M5 WP6 (RuntimeDeps has a new `nodes: Arc<dyn NodeLifecycle>` field; `NodeServices`, `NotifyingNodeLifecycle`, `ComfyServices::new(deps, runtime, node_lifecycle)`), M5 WP1 and WP2+WP3 (custom-node containment and Git/registry review), and package B (bandwidth limiter + HF relay in the download stack). The index already holds main's verified state; do not run git commands.

SPEC (authoritative): docs/handoff/plans/m6-approved-plan.md section "P - Signed updater and complete diagnostics"; HANDOFF.md §5.3 row P.

TASKS:
1. Resolve the conflict(s) in crates/desktop/src/state.rs keeping BOTH sides' behaviour: main's WP6 node lifecycle wiring and P's CollectDiagnostics/updater wiring. Construct any RuntimeDeps that P builds with the new `nodes` field using the same node lifecycle main already builds (no second NodeService/stack). Then make the whole workspace compile; fix any other semantic breakage from the newer baseline with the smallest change.
2. Close these plan gaps (plan: "Quiesce downloads and other background mutation before installer/exit; retain ownership through cleanup"; risk note: "Shutdown and flushing must precede installation"):
   a. Quiesce must also stop the model-library background scan/watch loop (and any other background writer that has a shutdown path), not only downloads, before the installer runs.
   b. Flush buffered app-log lines before calling the plugin's install() (on Windows install() can exit the process internally, so code after it may never run). If flushing requires dropping the tracing-appender guard, do it immediately before install() only and make sure a returned install error is still logged somewhere sensible; report what you chose.
   c. If install fails after quiescing, restore the stopped background work (downloads and library watch) so the app is usable without a restart, if the existing stacks support restarting cleanly; otherwise report the gap precisely.
   Add/extend tests in crates/desktop/src/updates/tests.rs that assert the order (quiesce of every background writer and the flush precede install; restore after a failed install).
3. Regenerate bindings: `cargo test -p desktop --test bindings --locked -j 4 -- --ignored export_bindings`, then make the UI compile and its tests pass (P's UI files are already applied: updates-panel, diagnostics-panel, comfy-page copy fallback, gateway).

SUCCESS CRITERIA (run at the end, report each result with counts):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test --workspace --locked -j 4 --no-fail-fast
- cargo test -p desktop --test bindings --locked -j 4 ; cargo test -p desktop --test acl --locked -j 4
- pnpm -C ui lint ; pnpm -C ui typecheck ; VITEST_MAX_WORKERS=2 pnpm -C ui test
- node --test scripts/check-arch-deps.test.mjs ; node scripts/check-arch-deps.mjs
- Confirm `grep -rn "<<<<<<<\|>>>>>>>" crates ui/src` finds nothing.

OUTPUT (final message): how the conflict was resolved (file:line), the gap fixes (file:line, chosen flush approach), tests added, exact command results, remaining gaps.

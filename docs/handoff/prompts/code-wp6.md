ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop and report the gap.

WORKING COPY: you are in a separate git worktree at C:\Users\vjump\vjc-wt\runtime (detached HEAD). Edit ONLY files inside this worktree. Do NOT touch C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager (another coder works there). Do not commit.

MANDATORY: read C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\coder-env.md (GNU toolchain env for every cargo command; repo rules). In Git Bash run `source /c/Users/vjump/AppData/Local/Temp/claude/C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager/ce429ed4-f6f3-49eb-9e87-13e9ba4d84af/scratchpad/gnu-env.sh` before cargo commands (sets GNU toolchain, CARGO_TARGET_DIR=target-gnu relative to this worktree, a test runner and link flags). The first build takes ~15 minutes.

GOAL: M5 backend finding 6 — ComfyUI start/restart node-journal checks must run under the held maintenance lease; restart must stop → reconcile/rescan → start under one lease; stop must not silently swallow the required rescan.

APPROVED SPEC (authoritative): C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\adv-m5-backend.out.md section "WP6 — Hold launch/restart checks and required rescanning within orchestration" — implement exactly: NodeLifecycle port in application comfy_runtime ports, RuntimeDeps.nodes, implementation for NodeService in new crates/application/src/custom_nodes/runtime.rs, RuntimeError::NodeRecoveryRequired and NodeRescanFailed with IPC codes node_recovery_required / node_rescan_failed, the Start/Restart/Stop/Update-with-stop ordering, StopComfy::new taking RuntimeDeps, removal of desktop pre-lease ensure_startable() and best-effort after_stop calls, a thin desktop NodeLifecycle wrapper that publishes NodeEvent::Changed after stop reconciliation (including failure), and startup reconcile+scan before exposing normal services. Add every test listed there (exact names) and keep existing gate-order/acquisition-count tests passing. Update all fakes/constructors (application tests, infrastructure tests, desktop tests/support) as needed.

Background evidence: C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\exp-a.out.md section "Finding 6".

SUCCESS CRITERIA (GNU env, inside this worktree):
- cargo fmt --all
- cargo clippy --workspace --all-targets --locked -- -D warnings
- cargo test -p application --locked; cargo test -p infrastructure --locked; cargo test -p desktop --lib --locked; cargo test -p desktop --test acl --locked — all pass. (cargo test -p desktop --test bindings may fail with pre-existing drift unrelated to you; report but do not regenerate bindings.)
- node scripts/check-arch-deps.mjs passes.

OUTPUT: summary per file (file:line), new public signatures, error variants and IPC codes, tests added (names), exact command results.

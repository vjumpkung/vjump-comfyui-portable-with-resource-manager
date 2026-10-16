ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop and report the gap.

WORKING COPY: separate git worktree C:\Users\vjump\vjc-wt\bandwidth (detached HEAD). Edit ONLY files inside it. Never touch C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager or other worktrees. Do not commit.

MANDATORY: read C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\coder-env.md. In Git Bash run `source /c/Users/vjump/AppData/Local/Temp/claude/C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager/ce429ed4-f6f3-49eb-9e87-13e9ba4d84af/scratchpad/gnu-env.sh` before every cargo command (GNU toolchain, CARGO_TARGET_DIR=target-gnu inside this worktree, test runner, link flags). First build ~15 min. Other builds run on this machine concurrently; prefer targeted test commands (-p infrastructure) over workspace-wide ones until the end.

GOAL: FR-DL-14 global bandwidth cap — one live shared budget for the native engine and the hf CLI engine.

APPROVED SPEC (authoritative): C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\adv-m6.out.md section "B — One live bandwidth budget for native and HF" (BandwidthLimiter API and semantics, per-attempt loopback CONNECT relay for every HF attempt incl. unlimited, allow-list with DNS-label boundaries, IP literal/userinfo/private-resolution rejection, bounded headers/connections/timeouts, relay lifetime bound to the child, child proxy env (upper+lower case) with bypass vars cleared, upstream proxy captured once from the parent env and chained (HTTP CONNECT to the upstream proxy; credentials never logged), error code bandwidth_proxy_unavailable not treated as ToolLaunch fallback, settings key unchanged). Implement every listed test except the opt-in real HF test (add it as #[ignore] with a clear doc comment).

INTEGRATION ALLOWANCE (main's decision, overrides the spec's "I owns" note for these small touch points so the package is complete and testable):
- crates/infrastructure/src/models/acquisition/mod.rs (module declarations/exports) and crates/infrastructure/Cargo.toml (tokio features such as "net" if needed; no new crates unless unavoidable — if you must add a crate, report it and update Cargo.lock with `cargo update -p <crate> --precise <ver>` style minimal change).
- crates/desktop/src/state.rs: pass the saved bandwidth cap into DownloadStack at composition; crates/desktop/src/ipc/commands.rs settings_set: after a successful save, call the shared limiter's set_limit (next to the existing set_concurrency call). Keep these edits minimal.
- ui/src/features/settings/settings-form.tsx (+ its test if any): enable the existing bandwidth field with integer KiB/s validation; empty = unlimited; 0 invalid; remove the "downloads run unthrottled" note. UI test name: bandwidth_empty_means_unlimited_and_zero_is_invalid.
- The acquisition error → IPC mapping for bandwidth_proxy_unavailable in the existing desktop download DTO/error mapping file.

SUCCESS CRITERIA (GNU env, in this worktree):
- cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings
- cargo test -p infrastructure --locked (all acquisition tests incl. new ones pass); cargo test -p desktop --lib --locked; cargo test -p desktop --test acl --locked
- pnpm -C ui install --frozen-lockfile (if node_modules missing), pnpm -C ui typecheck, pnpm -C ui lint, pnpm -C ui test
- node scripts/check-arch-deps.mjs

OUTPUT: per-file summary (file:line), public signatures, error codes, tests added (names) and exact results.

AMENDMENT (main, after your scope-gap report): you are authorized to add a `DownloadErrorCode` variant for the bandwidth relay failure in crates/domain/src/models/acquisition/mod.rs, update its persistence codec in crates/infrastructure/src/models/acquisition/codec.rs (stable persisted string `bandwidth_proxy_unavailable`), and update every exhaustive match/mapping that the compiler requires (application, infrastructure, desktop DTOs, and the UI download status/error text if it switches on codes). Add a codec round-trip assertion for the new code. Nothing else changes in scope.

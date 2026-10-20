ROLE: explorer (read-only). Do NOT edit files, do NOT run git write commands, do NOT run cargo/pnpm builds or tests. Use rg / file reads (LSP if available). Report evidence only; do not design solutions.

REPO: D:\vjump\vjump-comfyui-portable-with-resource-manager (Rust/Tauri 2 + React/TS desktop app managing ComfyUI). HEAD a4175d1.

QUESTION (bounded): Build the evidence map for a release-acceptance ledger.

1. From docs/REQUIREMENTS.md list EVERY requirement id (FR-*, NFR-*) with its priority (P0/P1/P2 as written) and a one-line summary. Keep the document order.
2. For every P0 and P1 requirement: find the implementing code (crate/module, file:line of the main symbol) and the automated tests that exercise it (exact test function names with file:line; Rust tests in crates/**, Vitest tests in ui/src/**/*.test.ts(x)). Classify the evidence: `unit/integration` (runs by default), `opt-in real` (#[ignore] real-system tests such as crates/infrastructure/tests/real_setup.rs — list their env vars), `none`. Note when the requirement is only partly covered (say which part).
3. Map each of the 17 acceptance scenarios in docs/REQUIREMENTS.md §8 to the closest existing automated tests (names + file:line) and say what part still needs a manual/native run (e.g. NVIDIA GPU, network/provider tokens, Linux, packaging).
4. List all #[ignore] tests in the workspace with their file:line and what they need to run.
5. Cross-check docs/IMPLEMENTATION_STATUS.md and docs/handoff/evidence/s3-residuals.md (residuals 1-21): list each residual id with a one-line summary and whether it is still present in the code (file:line evidence) or resolved.

OUTPUT: Markdown with sections 1-5, tables where natural, every claim with file:line. State remaining uncertainty explicitly. Be complete rather than short; this is the input for docs/RELEASE_ACCEPTANCE.md.

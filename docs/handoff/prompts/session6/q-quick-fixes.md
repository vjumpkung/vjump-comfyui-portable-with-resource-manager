ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If the plan is underspecified or conflicts with the code, stop that item and report the gap precisely instead of improvising.

## Build environment and repository rules (mandatory)
- WORKING COPY: `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`, tree clean at start). Other agents may be reading the tree; only you write. Never revert files you did not change for this package.
- Read `AGENTS.md` and `docs/handoff/tooling/coder-env.md` first. Default MSVC toolchain from `rust-toolchain.toml`, default `target/`; do NOT set RUSTUP_TOOLCHAIN or CARGO_TARGET_DIR.
- Memory: 32 GB machine, ~10 GB free. Use `-j 6`; targeted runs while iterating; workspace-wide commands once at the end. Never run two cargo commands at once. Vitest with `VITEST_MAX_WORKERS=2`.
- Rust: rustfmt, clippy `-D warnings`; TypeScript: 2-space, single quotes, no semicolons. Match surrounding style and comment density. Normalize edited files to LF.
- Do not commit, push, stash, reset, checkout, apply, clean or touch git history. Do not modify files outside the stated scope. Do not touch `D:\vjc-s5\` or `D:\comfyui-test\`, and do not kill `vjumpkung-comfyui-desktop` processes (the owner's debug app may be running from `target\debug`; do NOT run `cargo build -p desktop` without `--tests`/test commands — building the desktop binary would fail on the locked exe).

## Q1 (P0) — Python 3.12 cannot start ComfyUI on Windows
Root cause (reproduced by main): the ComfyUI child gets `tool_environment(layout, Tool::Python)` (`crates/infrastructure/src/environment/layout.rs:120-171`). `USER_VARIABLES_WINDOWS` lacks `USERNAME`. Python 3.12 `getpass.getuser()` checks LOGNAME/USER/LNAME/USERNAME, then `import pwd` → `ModuleNotFoundError` on Windows; torch 2.10's `torch._dynamo` import calls it (via comfy_kitchen) and ComfyUI dies with `AssertionError: Artifact of type=precompile already registered`. Python 3.13 raises `OSError` instead, which torch tolerates. With USERNAME present ComfyUI 0.42.1 starts (verified).
1. Add `"USERNAME"` to `USER_VARIABLES_WINDOWS` (array length 7 → 8) with a short comment: Python 3.12's `getpass.getuser()` (used by torch) falls back to the Unix-only `pwd` module without it; the user name is already part of USERPROFILE.
2. Test in the existing `layout.rs` tests module: `#[cfg(windows)] #[test] fn python_children_get_the_windows_user_name()` — if the parent has `USERNAME`, `tool_environment(&layout, Tool::Python)` contains `USERNAME` with the parent's value (skip with an early return if the parent lacks it). `arbitrary_parent_variables_are_not_inherited` must keep passing unchanged (it derives the allow-list from the arrays).
3. Do NOT change `shared/process.rs::system_baseline` or the test `the_child_only_sees_the_given_environment` (a baseline-only child must still not see USERNAME).

## Q2 — document scroll / shell overflow (Settings page)
Observed in the running app (dark theme, Settings scrolled to bottom): the whole document scrolls in addition to `main`; the sidebar and its border stop ~2/3 down; an empty band shows below; two scrollbars. Shell: `ui/src/app/shell/app-shell.tsx:108` root `flex h-screen …` (no overflow containment), `:110` column `flex min-w-0 flex-1 flex-col`, `:112` `<main className="flex-1 overflow-y-auto p-6">` (not positioned). Cause (accepted hypothesis): an absolutely positioned descendant (e.g. Tailwind `sr-only`, Radix hidden inputs) has the initial containing block as containing block, so `main`'s overflow does not clip it and it extends the document.
1. Root (`:108`): add `relative` and `overflow-hidden` (keep existing classes).
2. Column (`:110`): add `min-h-0`.
3. Main (`:112`): `relative min-h-0 flex-1 overflow-y-auto p-6`.
4. Do NOT add global `html/body` overflow rules (`App.tsx` `SetupFrame` uses `min-h-screen` and relies on document scroll).
5. Vitest in `ui/src/app/shell/app-shell.test.tsx` (extend, exact name): `shell_contains_overflow_inside_main` — the shell root has `overflow-hidden` and `relative`; `main` has `relative`, `min-h-0`, `overflow-y-auto`.

## Q3 — active-row colour contrast (Models and Custom Nodes tables)
Observed: the active row is `bg-accent` (#ff9fed, pink) with inherited light text: dark theme #e0e5ec on #ff9fed = 1.45:1, muted #b8c0c9 = 1.0:1; outline badges and the pink Conflict chip disappear. Files: `ui/src/features/library/model-table.tsx:157` (`key === activeKey ? 'bg-accent' : 'hover:bg-muted/50'`), `ui/src/features/custom-nodes/installed-table.tsx:143` (`node.id === activeId ? 'bg-accent' : 'even:bg-muted/45 hover:bg-muted'`). Both rows set `aria-selected={isSelected}` (checkbox state) which is unstyled.
1. Active row in both tables: `bg-muted shadow-[inset_3px_0_0_var(--accent)]` (no brand fill behind table text). Non-active rows keep their current classes, and additionally a checked (selected) non-active row gets `bg-muted/60` (use the existing `isSelected` value in each component; keep zebra/hover for unchecked rows). Contrast: dark #e0e5ec/#b8c0c9 on #3b4652 ≈ 8.5:1/5.9:1; light #2a313a/#56606c on #d2d8e0 ≈ 10:1/4.6:1.
2. Do not change table headers, chips, badges or tokens in `globals.css`.
3. `docs/DESIGN.md` Tables section (lines ~99-102): add one bullet: "Active row: `bg-muted` with a 3px accent inset bar (`shadow-[inset_3px_0_0_var(--accent)]`); checked rows `bg-muted/60`. Never put a brand fill behind table text (contrast)."
4. Vitest (extend the existing test files, exact names): `model_table_active_row_keeps_readable_contrast` in the library model-table/library-page test and `installed_table_active_row_keeps_readable_contrast` in `installed-table.test.tsx` — the active row has `bg-muted` and not `bg-accent`.

## Success criteria (run at the end; report exact counts)
```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings
cargo test -p infrastructure --locked -j 6 layout
cargo test --workspace --locked -j 6 --no-fail-fast
pnpm -C ui lint
pnpm -C ui typecheck
VITEST_MAX_WORKERS=2 pnpm -C ui test
pnpm -C ui build
node scripts/check-arch-deps.mjs
```
Expected baseline before your change: Rust 718 passed / 0 failed / 10 ignored; Vitest 29 files / 184 tests.
OUTPUT: per-file summary with file:line, key diffs, every command result with counts, deviations with reasons. Also write it to `docs/handoff/evidence/s6-q-implementation.md`.

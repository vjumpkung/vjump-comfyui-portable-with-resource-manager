# Session 6, package Q implementation report

Branch `feat/m0-skeleton`, HEAD `e384d43`. No git write commands were used. The owner's running exe (`target\debug\vjumpkung-comfyui-desktop.owner-running-1159.exe`) was not touched and no `cargo build -p desktop` was run.

## Per-file summary

### Q1 (P0) Python 3.12 ComfyUI start on Windows
- `crates/infrastructure/src/environment/layout.rs:121-124` — `USER_VARIABLES_WINDOWS` grows from `[&str; 7]` to `[&str; 8]` with `"USERNAME"` added (first entry). Doc comment added above the const explaining Python 3.12 `getpass.getuser()` / `pwd` fallback and that the user name is already part of `USERPROFILE`.
- `crates/infrastructure/src/environment/layout.rs:352-364` — new `#[cfg(windows)] #[test] fn python_children_get_the_windows_user_name()`. Early return if the parent has no `USERNAME`; otherwise `tool_environment(&layout, Tool::Python)` must contain `USERNAME` with the parent's value. The test ran with `USERNAME` present on this machine (it is not just the early-return path; the machine has the variable set).
- Not changed: `shared/process.rs::system_baseline`, `the_child_only_sees_the_given_environment`, `arbitrary_parent_variables_are_not_inherited` (still passes unchanged since it derives the allow-list from the arrays).

### Q2 Shell overflow
- `ui/src/app/shell/app-shell.tsx:108` root: `relative flex h-screen overflow-hidden bg-background text-foreground`.
- `ui/src/app/shell/app-shell.tsx:110` column: `flex min-h-0 min-w-0 flex-1 flex-col`.
- `ui/src/app/shell/app-shell.tsx:112` main: `relative min-h-0 flex-1 overflow-y-auto p-6`.
- No global `html/body` rules added.
- `ui/src/app/shell/app-shell.test.tsx:111` — new `shell_contains_overflow_inside_main`: root (`container.firstElementChild`) has `overflow-hidden` + `relative`; `getByRole('main')` has `relative`, `min-h-0`, `overflow-y-auto`.

### Q3 Active-row contrast
- `ui/src/features/library/model-table.tsx:157-161` — active: `bg-muted shadow-[inset_3px_0_0_var(--accent)]`; else checked: `bg-muted/60`; else `hover:bg-muted/50`.
- `ui/src/features/custom-nodes/installed-table.tsx:143-148` — active: `bg-muted shadow-[inset_3px_0_0_var(--accent)]`; else checked: `bg-muted/60 hover:bg-muted`; else `even:bg-muted/45 hover:bg-muted`.
- `docs/DESIGN.md:103` — Tables bullet "Active row: ..." added (text as specified).
- `ui/src/features/library/library-page.test.tsx:414` — new `model_table_active_row_keeps_readable_contrast` (click row `a.pt`; it has `bg-muted`, not `bg-accent`; the other row has no `bg-accent`).
- `ui/src/features/custom-nodes/installed-table.test.tsx:162` — new `installed_table_active_row_keeps_readable_contrast` (active row `a` has `data-active=true`, `bg-muted`, not `bg-accent`; row `b` has no `bg-accent`).
- Headers, chips, badges, `globals.css` untouched.

## Commands and results (all from the repo root)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings |
| `cargo test -p infrastructure --locked -j 6 layout` | 6 passed, 0 failed (incl. `python_children_get_the_windows_user_name`, `arbitrary_parent_variables_are_not_inherited`, `environment_is_an_allow_list_with_isolated_caches`, `paths_follow_the_documented_layout`) |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | exit 0; summed over all `test result` lines: 719 passed / 0 failed / 10 ignored (baseline 718 / 0 / 10, +1 new test) |
| targeted vitest (app-shell, installed-table, library-page) | 3 files, 25 tests passed |
| `pnpm -C ui lint` | exit 0 |
| `pnpm -C ui typecheck` | exit 0 |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 29 files passed / 187 tests passed (baseline 29 / 184, +3 new tests) |
| `pnpm -C ui build` | exit 0 (only the existing chunk-size warning) |
| `node scripts/check-arch-deps.mjs` | `Architecture dependencies: OK` |

## Deviations
- None from the spec. Notes:
  - `USERNAME` is placed first in the array (order is irrelevant to behavior).
  - Selected non-active rows use a ternary instead of appending a class, so the zebra (`even:`) variant cannot override `bg-muted/60` on even rows; unchecked rows keep their exact previous classes. In the Models table a checked row has no extra hover class (unchecked keep `hover:bg-muted/50`); in the Custom Nodes table a checked row keeps `hover:bg-muted`.
  - `ui/src/features/library/model-table.tsx` had CRLF line endings in the working copy (index is LF, `* text=auto`); it is now LF as instructed, so it does not appear as a whole-file change in `git diff`.
  - Visual verification of the Q2/Q3 CSS in the running app was not performed (the tests assert classes only).

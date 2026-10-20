# Session 6 — package F (setup automation) implementation report

Coder: Sonnet 5.5 (fallback coder). Spec: `docs/handoff/prompts/session6/f-setup-automation.md`. Working copy `D:\vjump\vjump-comfyui-portable-with-resource-manager` (branch `feat/m0-skeleton`, HEAD `e384d43`, Q and S0 changes preserved). Default MSVC toolchain and `target/`; `-j 6`; Vitest with `VITEST_MAX_WORKERS=2`. No git write commands were used.

## Per-file summary

### F1 — third-party consent in first-run setup
- `crates/application/src/environment/model.rs:327-338` — `SetupRequest.third_party_consent: bool`; `curated_nodes` doc comment now says a first-run request may carry the review-stage consent.
- `crates/application/src/environment/error.rs:37-38` — `EnvironmentAppError::ThirdPartyConsentInvalid` ("third-party consent can only be given for a first-run setup").
- `crates/desktop/src/ipc/error.rs:181-184` — maps it to code `third_party_consent_invalid`, message "Third-party consent can only be given for a first-run setup."; test `invalid_third_party_consent_has_a_fixed_ipc_code_and_message` (`:388-396`).
- `crates/application/src/environment/setup.rs:72-74` — first check in `StartSetup::register`: flag true and mode != `FirstRun` -> the new error. `:150` Rebuild derivation sets `request.third_party_consent = false`. The Repair derivation (`:158-197`) copies only `profile_id`, `accelerators` and `install_manager` from the recorded run, never the whole request, so it needed no change. `curated_progress` doc comment updated (`:292-294`).
- `crates/infrastructure/src/environment/sqlite.rs:485-486` (`#[serde(default)] third_party_consent: bool` on `RequestDoc`), `:614` (encode), `:791` (decode). The request is stored as JSON, so no migration.
- `crates/application/src/environment/execute.rs`
  - `manager()` (`:1083-1101`): with the flag and an unapproved resolved commit it sets `run.manager_consent`, saves the run, logs "ComfyUI-Manager {url} at commit {commit} was approved on the setup review page." and continues; without the flag it still returns `ConsentRequired`.
  - `curated_nodes()` (`:1123-1235`): `decisions` is mutable; `auto = request.third_party_consent`; after `nodes.pending()`, every Prepared entry whose review has no `protected_changes` and no covering decision gets `setup_review_decision(review)` (`:1571-1586`: `execute_node_code: true`, `acknowledge_warnings: true`, everything else false, `protected_changes: []`, bound to the plan id and binding). Each warning is first logged as "{name}: {warning} (acknowledged on the setup review page)."
  - `const MAX_CURATED_PASSES: usize = 3` (`:50`); a fourth pass in auto mode fails the step with "The curated node plans kept changing. Retry setup."
  - No "Skipped" path for protected changes (they fail at prepare); a plan with protected changes is never answered, so it falls into the existing pause.
- `crates/desktop/src/ipc/env_dto.rs:921-924` (`SetupRequestDto.third_party_consent`, wire `thirdPartyConsent`, required), `:962` (`into_application`).
- `ui/src/shared/ipc/bindings.ts` — regenerated with `cargo test -p desktop --test bindings --locked -- --ignored export_bindings` (adds `thirdPartyConsent`, drops `branch`/`commit` from `CuratedNodeDto`).
- `ui/src/features/setup/setup-wizard.tsx`
  - `:56-59` constants `MANAGER_SOURCE` (display only) and `THIRD_PARTY_DISCLOSURE` (exact spec text).
  - `:221-234` `thirdPartyConsent = mode === 'first_run' && (installManager || (curatedSelection ?? []).length > 0)` and the source list (Manager first, then each selected curated repository URL).
  - Review stage `:912-928`: `Alert` "Third-party code" with the disclosure and a `ul[aria-label="Third-party sources"]`, shown only when the flag is true.
  - `:1012` request sends `thirdPartyConsent`.
  - Copy: Manager help `:749-752`, curated section `:806-811`, row `:836` ("{repository} · latest commit of the default branch"), review dd `:886-891`; the sentence "Curated selection is not execution consent…" is removed. `run-progress.tsx` is untouched (still serves runs without the flag).
- `ui/src/features/setup/rebuild-setup.tsx:46` — `thirdPartyConsent: false`.

### F2 — curated nodes at the latest default-branch commit
- `crates/infrastructure/src/environment/curated.rs` — `SCHEMA_VERSION = 2`; `EntryDoc { repository, #[serde(default)] unavailable }` with `deny_unknown_fields`; branch/commit validation, `CuratedError::{Branch, Commit}` and the `valid_ref_name` import removed.
- `crates/infrastructure/src/environment/data/curated_nodes.json` — schema 2, 22 entries `{ "repository" }` in the same order (PlagueKind adds `unavailable`), new provenance and PlagueKind reason exactly as specified.
- `crates/application/src/environment/model.rs:475-485` — `CuratedNodeSpec` loses `branch`/`commit`. `crates/desktop/src/ipc/env_dto.rs:337-352` — `CuratedNodeDto` loses them.
- `crates/application/src/custom_nodes/setup.rs`
  - `:21-27` `expected_repository(entry) -> String` (same trimmed form as before: no trailing `/`, no `.git`).
  - `:29-39` `is_curated_source` accepts `NodeSource::Git { repository, commit, .. }` with that repository and a 40-hex lowercase commit, any branch.
  - `:236` `PrepareRequest::Git { reference: None }`.
  - `:250` refusal text "The repository did not resolve to the curated repository's default branch." (the plan is discarded and NotApplied is returned).
  - `installed()` (`:326-338`) matches a Git node by repository and keeps the conflict, single-file, digest and dependency checks. `verify_curated` inherits this.
- `crates/application/src/environment/execute.rs` — log "Preparing {name} from the latest commit of its default branch." and, after a successful prepare, "{name}: {branch|default branch} at commit {short10}." (`:1327-1345`); doc comments at `:1118-1122` and `:1258-1259` updated.

### F3 — recommended profile
- `crates/infrastructure/src/environment/data/profiles/win-py312-torch2100-cu130.json` — `recommended: true`, label "Python 3.12, PyTorch 2.10.0, CUDA 13.0"; `win-py313-torch2141-cu130.json` — `recommended: false`. No revision bump, `calibrated: false` kept, Linux unchanged.
- `crates/infrastructure/src/environment/profiles/tests.rs:24,71-97` — per-OS recommended id and Python (Windows `win-py312-torch2100-cu130` / 3.12.15, Linux `linux-py313-torch2141-cu130` / 3.13.16); the calibrated-comment now names the py313 Windows profile.

### Tests added or updated
- Application (`crates/application/src/...`):
  - `environment/tests/setup.rs`: `first_run_consent_installs_manager_without_pausing`, `third_party_consent_is_refused_outside_first_run` (Repair, ChangeTorch, ChangePython, SwitchVenv, Rebuild refused, no run stored; FirstRun accepted), extra `curated_auto_consent_stops_when_plans_keep_changing` (World fake, 5 injected `PlanStale`; exactly 3 prepare/commit passes then the specified failure), `Curated::register_consenting`, `request()` gets the field.
  - `environment/tests/relocation.rs`: `rebuild_never_inherits_third_party_consent`.
  - `custom_nodes/tests.rs` (module `curated_setup`): `curated_auto_consent_installs_every_node_and_logs_warnings`, `curated_auto_consent_resynthesizes_after_restart`, `curated_setup_prepares_the_default_branch_head`, `curated_repository_mismatch_never_publishes` (replaces `curated_commit_mismatch_never_publishes`; refuses another repository, uppercase, short and empty commits). The fake `FakeFiles` now returns `branch: Some("main")` plus a resolved commit when `reference` is `None` (`default_head`, `repository_override`, `references` fields replace `commit_override`).
  - `environment/tests/fakes.rs`: `curated_spec(id)`, `World` curated review `branch: Some("main")`, `RecordingSink::log_lines`.
- Infrastructure: `environment/sqlite/tests.rs` `run_doc_without_third_party_consent_reads_false` (round-trips true, then a stored JSON without the field reads false); `environment/curated/tests.rs` rewritten (22 entries in reference order, repository only, schema 2, malformed cases incl. unknown `commit` and `branch` fields refused, only PlagueKind unavailable with the new reason).
- Desktop: `ipc/env_dto.rs` test asserts the DTO flag maps through; `tests/acl.rs` `setup_run` body gains the required `thirdPartyConsent: false`; `state/recovery_tests.rs` literal.
- UI: `setup-wizard.test.tsx` `review_page_discloses_third_party_code_and_sends_consent` (first_run curated-only and Manager-only -> disclosure, exact source list, `thirdPartyConsent: true`; Manager plus one curated -> Manager first; nothing selected -> no disclosure, `false`; change_torch with Manager and repair -> no disclosure, `false`); existing wizard tests updated for the new field and catalog shape; `App.test.tsx`, `gateway.test.ts` updated.

### Docs
- `docs/REQUIREMENTS.md` — NFR-SEC-3 first-run exception, FR-SETUP-9, §2.4 table and sentence, §5.4 profile list. (File was CRLF in the working copy; normalized to LF, so the diff against HEAD is only the edited lines.)
- `docs/DOCUMENTATION.md` — profile table and notes, Manager option, curated option, Review stage, "When a step fails", limitations bullet.
- `HANDOFF.md` §3 — Python/torch row amended (Windows default py312/torch 2.10) and the new owner-decision row.
- `docs/handoff/plans/m6-approved-plan.md` — one-line "Superseded for first-run setup…" pointer under "Consent and execution".

## Commands (all from the repository root; final state)
| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -j 6 -- -D warnings` | exit 0, no warnings |
| `cargo test -p application --locked -j 6 curated` | 24 passed, 0 failed, 181 filtered |
| `cargo test -p application --locked -j 6 third_party` | 2 passed, 0 failed, 203 filtered |
| `cargo test -p infrastructure --locked -j 6 curated` | 5 passed, 0 failed (+ 0-test targets) |
| `cargo test -p infrastructure --locked -j 6 profiles` | 9 passed, 0 failed |
| `cargo test -p infrastructure --locked -j 6 third_party_consent` | 1 passed (the sqlite test) |
| `cargo test -p desktop --test bindings --locked` | 1 passed, 1 ignored (`bindings_up_to_date` ok; `export_bindings` run explicitly to regenerate) |
| `cargo test -p desktop --test acl --locked` | 11 passed, 0 failed |
| `cargo test --workspace --locked -j 6 --no-fail-fast` | 731 passed, 0 failed, 9 ignored: application 205; desktop lib 50; bin 0; acl 11; bindings 1 (+1 ign); navigation 2; stack_reserve 2; startup_smoke 0 (+1 ign); domain 45; infrastructure lib 406 (+7 ign); real_setup 0 (+1 ign); workflow 9; doc-tests 0 |
| `pnpm -C ui lint` | exit 0, no findings |
| `pnpm -C ui typecheck` | exit 0 |
| `VITEST_MAX_WORKERS=2 pnpm -C ui test` | 29 files, 188 tests passed |
| `pnpm -C ui build` | built (only the existing chunk-size warning) |
| `node scripts/check-arch-deps.mjs` | "Architecture dependencies: OK" |

Mutation check: with `auto` forced to `false` in `curated_nodes()` the three `curated_auto*` tests fail, so they exercise the new path (reverted afterwards; `execute.rs` diff unchanged).

## Deviations and findings
1. **Manager copy for non-first-run modes.** The Manager checkbox is also shown in change_torch/change_python/switch_venv, and those modes still pause for Manager consent. A literal replacement would have made the help text (`setup-wizard.tsx:749-752`) and the review dd (`:886-891`) wrong there, so the new text is used for `first_run` only and the old wording ("setup pauses for consent…", "Manager selected; consent still required") is kept for the other modes. Verified in the Vitest test.
2. **`App.test.tsx` first-run request.** The spec said to add `thirdPartyConsent: false`, but the test selects a curated node in a first run, so the wizard now really sends `true`; the test expects `true`. The rebuild request in the same file expects `false`.
3. **Files outside the listed scope that had to change to compile or pass:** `crates/desktop/tests/acl.rs` (the `setup_run` body must carry the required wire field, otherwise the ACL test fails with "missing field `thirdPartyConsent`"), `crates/infrastructure/tests/real_setup.rs`, `crates/infrastructure/tests/workflow.rs`, `crates/desktop/src/state/recovery_tests.rs`, `crates/application/src/environment/tests/fakes.rs`, `.../tests/relocation.rs` (struct literals / catalog shape / new test).
4. **Repair derivation.** The spec asked to clear the flag in "any Repair derivation that copies a recorded request"; the Repair path in `StartSetup::register` does not copy the recorded request (it copies three fields), and a client-sent flag for Repair is refused by the new first check. Only the Rebuild derivation needed the line.
5. **Editor hook caveat (process, not product).** The harness runs a `rustfmt` PostToolUse hook after Edit/Write on `.rs` files using a different style than `cargo fmt` (edition 2021 import order, macro layout), which churned many unrelated lines in the files I edited. I ran `cargo fmt --all` (repo edition 2024) after the last Rust edit; `cargo fmt --all -- --check` passes and `git diff` of the Rust files contains only intended hunks. Any later Edit/Write of a `.rs` file will trigger the hook again, so run `cargo fmt --all` last.
6. **Unreachable-by-design pause.** If a synthesized decision were ever refused by `commit` with `ConsentRequired`, or a plan carried protected changes, the step still falls back to the existing review pause (no new Skipped path, per the ruling that protected changes already fail at prepare). A normal auto run never reaches it.
7. Not done (out of scope per the spec): no change to the UI fixture profile (`shared/testing/environment.ts`), `run-progress.tsx`, `docs/DESIGN.md`, or other docs; no native/real-hardware run.

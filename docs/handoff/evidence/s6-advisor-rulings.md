# Session 6 — advisor stand-in rulings (2026-10-20)

Codex rate-limited until 14:36 → read-only Opus stand-ins (tested fallback). Plans reviewed:
`plans/s6-owner-feedback-plan.md` (Q, F, G, X) and `plans/s6-s0-stack-overflow.md` (S0). All questions AI-decidable;
the corrected plans are embedded verbatim in the coder prompts under `prompts/session6/`.

## S0 (P0 stack overflow) — GO with corrections
- Linker reserve alone suffices for the main thread (covers setup/shutdown `block_on` and sync commands); no extra
  thread. tokio moves `block_on` futures > 2 KiB to the heap in debug, so the overflow is poll-call depth, not future
  size; node walks already run in `spawn_blocking`.
- Also give tokio workers 8 MiB (the same `rescan` chain runs on workers for `node_rescan` and after ComfyUI stops):
  explicit runtime installed first in `run()` via `tauri::async_runtime::set`, runtime kept in a static.
- Tests: PE `SizeOfStackReserve` of the real bin ≥ 8 MiB and test exe < 8 MiB; worker/blocking thread limits via
  `GetCurrentThreadStackLimits`. Node-tree smoke test optional (likely would not fail pre-fix) → not shipped; native
  check on the owner root is the evidence.

## Q — GO (diff reviewed, correct). Optional nit: checked Models row has no hover style.

## F — GO with corrections
1. Consent = disclosure on the wizard's review stage + the **Start setup** click (button label is "Start setup", not
   "Install"); no checkbox. NFR-SEC-3 gets an explicit first-run exception; every URL@commit is logged before install.
2. Auto-acknowledging curated warnings is acceptable: setup prepares only Git sources; possible warnings are
informational (submodules, install.py not run); DETACHED disappears with DefaultHead; all warnings logged.
3. The "protected change → Skipped" branch is dead code: protected changes fail at prepare (`workflow.rs:465-469`),
   `protected_changes` is always empty (`:478`), commit refuses them (`:562-564`). Dropped. Prepare failures keep
   today's Retry/Skip semantics.
4. FirstRun only; Repair with Manager still pauses for consent (tell the owner).
5. Drop `branch` from the catalog (reference installer clones the default branch; rgthree's default moved dev→main).
6. F3: flip `recommended` without a revision bump, keep `calibrated: false`, fix the py312 label.
- Add `MAX_CURATED_PASSES = 3` guard against an endless PlanStale loop in auto mode.

## G — GO with corrected G1
- Restricted mode cannot be reused: its state has no `services()`, which the wizard needs (hardware, profiles,
  python validation). Instead keep normal services and only defer the model roots (download stack + library roots)
  and the node reconcile when the manifest is Absent AND `<root>/ComfyUI` does not exist; restart after a successful
  FirstRun (same path as Rebuild). Only those two constructors create anything under `ComfyUI/` at startup.
- G2: domain `checkout_placement` rules (no default branch / other branch / target not ancestor / local-only commits →
  detached with reason; else Branch(D)); apply `checkout -B D <commit>` + best-effort `--set-upstream-to`; used by the
  setup clone (also attaches the Manager clone to `main`) and by the app update's non-ff arm. Compatible with
  FR-COMFY-6 because it runs only on a clean tree with no unpublished local commits. Older tag keeps the branch.

## X — GO with corrections
- Separate `ExtraModelPathsWriter` port (read-only consumers untouched); `is_default: false`; app downloads stay in
  `ComfyUI/models` (documented limitation).
- Backups go to `<root>/data/backups/extra_model_paths-<UTC>.yaml`, never beside the file (a sibling `.bak` would
  dirty the ComfyUI checkout and make app updates refuse).
- Fail closed on files that cannot be rewritten safely (link, >1 MiB, non-UTF-8/malformed, non-mapping top level,
  `<<` merge keys); base-path validation (absolute local/UNC, no `\\?\`/`\\.\`, no `$`/`%`/leading `~`/control chars,
  outside ComfyUI/envs/data/tools and not the root, no reparse point in the existing prefix).

## Order
S0 → F → G → X → D3+D5 → D4 → D1 → R (F before G: owner's main request; G touches startup composition and needs the
mandatory smoke test).

## Owner follow-up (optional, non-blocking)
Should app downloads and ComfyUI's default write folder (`is_default: true`) use the chosen base folder? Advisor
recommendation: keep current behaviour for Release 1; a separate package can add a selectable download root (widens
NFR-SEC-5 write authorization).

## G2b (native finding N12: patch tags live on release branches) — option (a), AI-decidable
Evidence (read-only on `D:\vjc-s6\r1\ComfyUI`): `.0` tags (v0.42.0, v0.41.0, v0.40.0, v0.39.0, v0.36.0) are ancestors of
`origin/master`; patch tags only on `release/vMAJOR.MINOR` (v0.42.1 → release/v0.42; v0.41.1–3 → release/v0.41;
v0.39.2, v0.38.2, v0.37.4, …); older commits are in several release branches, so the branch name is derived from the
tag (`release_line`), not from `--contains`; full clones and `fetch --tags` already carry `origin/release/*`.
Rule: default branch when it contains the target (never fall through), else the tag's release line when it contains
the target, each only with no unpublished local commits; else detached. Prompt: `prompts/session6/g2b-release-branch.md`
(+ UI polish: recommended profile listed first).
Owner-only (non-blocking, ask at the end): what should `git pull` bring in a release install? (a) patches of the
installed line [recommended, implemented]; (b) always `master` (changes §2.4 default — not recommended);
(c) additionally a wizard choice "Development (master)" (separate package if wanted).

## U (owner 14:33: update by git pull to latest commit OR checkout a tag) — Codex advisor gpt-6.1-sol xhigh: GO with corrections
Full ruling: `evidence/s6-advisor-u.md`. New `UpdateTarget::LatestCommit`; `FrozenUpdate.branch` persisted as
`target_branch` in the JSON update document (no migration); one `FrozenUpdate::policy_target()` validation used by
`finish` and `apply`; LatestCommit fetches `origin refs/heads/*` explicitly; apply uses the persisted branch; Intent +
LatestCommit applies even at the same commit; reference = tag or actual branch. Wizard "Latest commit (master)" choice =
owner-only, deferred (ask at the end). Prompt: `prompts/session6/u-update-modes.md`.

## D3b (native finding on the robocopy copy) — main, AI-decidable correction
uv recreates the minor-version links of every managed Python on each `uv python install`, so a copied
`cpython-3.12-…` folder broke the install of 3.13.16 after D3 had cleared only the 3.13 link. Correction: clear every
pinned minor-link name for the platform with the same safety rules. Prompt: `prompts/session6/d3b-all-minor-links.md`.

## N + D1b (owner: allow every curated node in consented setup; install.py → requirements.txt everywhere; page = order only) — Codex advisor gpt-6.1-sol xhigh: GO for one corrected package
Full ruling: `evidence/s6-advisor-n.md`. Internal `NodePreparePolicy { Strict, ConsentedFirstRun }` derived from persisted
FirstRun consent + selected curated repository (Active/page always Strict); only shipped `__pycache__` and D1 top-level
native images are relaxed (as plan warnings), all containment/provenance/Git-config/preflight/limit checks kept;
Remove PlagueKind's `unavailable` field while keeping all 22 entries unticked and preserving future unavailability
support. Complete D1 item 3 with a raw stdout process API (`run_raw_stdout`, 8 MiB Git cap,
exact bytes, never logged) and a strict Windows pre-checkout `ls-tree -z` admission check. Hook-first order with pin
reassertion, profile-constraint restore and a second constrained preview of the frozen reviewed InstallSpec before
installing requirements. The post-hook preview must reuse guarded reviewed bytes and original exact package
versions, stop on protected drift/preflight failure, and never discover hook-generated requirements when none
were reviewed. Restore pins after failure/cancellation too. `allow_source_builds` stays false: app-managed
resolution/install remains wheel-only, but arbitrary approved scripts may invoke their own installers/builds.
The skipped-hook warning/note applies only when setup will not run the hook. Both wizard disclosures state the
cache/native exceptions and hook-before-reviewed-requirements order; the page's hook checkbox states this order
and remains unchecked. Manager retains its separate source/requirements path. Log full repository URL and commit
before commit. The two exact deduplicated content warnings are:
- `This repository ships __pycache__ content; consented first-run setup allows it.`
- `This node has top-level native-image entries that tools may load; consented first-run setup allows them.`

No user-only question remains. Residual #6 closes before consent only after the two Windows admission/cleanup
tests pass alongside pinned-uv isolation evidence; post-consent and installed/user-placed-node exposure remains.
Do not close residual #28 from those native-refusal tests. Prompt: `prompts/session6/n-d1b-node-policy.md`.

# Session 6 — owner feedback plan (draft by main, for advisor review)

Date: 2026-10-20 (~12:40 +07:00). HEAD `e384d43` (owner commit = all session-5 work through D2), working tree clean.
Codex is rate-limited until 14:36; advisor = read-only Opus stand-in, coder = Sonnet xhigh fallback (tested in sessions 4/5).

## Owner request (verbatim intent)
1. UI: (a) Settings page "overflowing" — the whole document scrolls besides the inner `main` scroll; the sidebar and its
   border stop ~2/3 down; an empty band shows below. (b)+(c) colour contrast — the active row in the Models and Custom
   Nodes tables is bright pink (`bg-accent` #ff9fed) with inherited light text (dark theme 1.45:1; muted text 1.0:1).
2. Profile Python 3.12 + torch 2.10 fails to start ComfyUI (owner install `D:\comfyui-test\v1`, log
   `data\logs\comfyui-20261020T050302135Z-…log`).
3. Feedback: (a) installing the curated custom nodes and ComfyUI-Manager in FIRST-TIME setup must not ask for
   permission — the flow must run automatically; (b) curated nodes must be `git clone`d at the LATEST commit of the
   repository (no commit lock); (c) if py3.12 + torch 2.10 works, make it the recommended profile.
4. New feature: `extra_model_paths.yaml` — the user picks a base path, the app generates the rest automatically.
5. Then continue the pending packages (D3+D5, D4, D1), native acceptance R, and generate README.md + documentation.

## Evidence gathered by main / explorers
- **P0 root cause (2), reproduced:** the ComfyUI child gets `tool_environment(layout, Tool::Python)`
  (`crates/infrastructure/src/environment/layout.rs:120-171`); `USER_VARIABLES_WINDOWS` passes USERPROFILE, APPDATA,
  LOCALAPPDATA, HOMEDRIVE, HOMEPATH, TEMP, TMP but **not USERNAME** (Unix list has USER/LOGNAME). Python 3.12
  `getpass.getuser()` checks LOGNAME/USER/LNAME/USERNAME then does `import pwd` → `ModuleNotFoundError` on Windows
  (3.13 converts that into `OSError`, which torch tolerates). `comfy_kitchen` → `torch._dynamo` import aborts half-way;
  the second import hits `AssertionError: Artifact of type=precompile already registered`. Reproduced with `env -i`
  + the owner's env python: without USERNAME `import torch._dynamo` fails with `No module named 'pwd'`; with
  USERNAME it imports. With the app's exact environment + USERNAME, `main.py --quick-test-for-ci` (user/temp/in/out
  redirected to scratch) exits 0: Python 3.12.15, torch 2.10.0+cu130, `Device: cuda:0 NVIDIA GeForce RTX 3060`,
  ComfyUI 0.42.1, ComfyUI-Manager V3.42 loads. (Owner install has only Manager, no curated nodes.)
  USERPROFILE already exposes the user name, so adding USERNAME discloses nothing new.
- **UI (explorer):** shell `ui/src/app/shell/app-shell.tsx:108` `flex h-screen` (no overflow-hidden), `:112`
  `<main className="flex-1 overflow-y-auto p-6">` (not positioned). No html/body/#root sizing in `globals.css`.
  No escaping element identified statically; hypothesis: an absolutely positioned descendant (e.g. Tailwind `sr-only`,
  Radix visually-hidden input) whose containing block is the initial containing block is not clipped by `main`'s
  overflow and extends the document. Active rows: `ui/src/features/library/model-table.tsx:157` and
  `ui/src/features/custom-nodes/installed-table.tsx:143` use `bg-accent` without `text-accent-foreground`; children use
  `text-muted-foreground`, outline badges (`text-foreground`), and a pink Conflict chip. DESIGN.md:39 "text on a brand
  fill is always #2a313a"; no spec for an active row. Headers (cyan + #2a313a) pass (~11:1).
- **Setup consent (explorer):** Manager gate `crates/application/src/environment/execute.rs:1080-1087`
  (`run.manager_consent != Some(resolved.commit)` → `ConsentRequired`); Manager source = `ls-remote HEAD` of
  `pins.rs:73 MANAGER_URL` (not pinned); `run.manager_consent` persisted. Curated: `execute.rs:1106-1209`
  (`prepare_curated` → pause at `:1137` unless every Prepared plan is covered by a process-local decision, `covers()`
  `:1505-1512` = plan/binding match, `execute_node_code`, warnings acknowledged, no protected changes).
  Setup decisions are forced to `run_install_py=false`, no remember, no source builds
  (`crates/application/src/custom_nodes/setup.rs:140-154`). Catalog
  `crates/infrastructure/src/environment/data/curated_nodes.json` (schema 1, 22 entries with repository/branch/
  40-hex commit; PlagueKind `unavailable`), parser `curated.rs`; prepare uses
  `PrepareRequest::Git { repository, reference: Some(entry.commit) }` and requires
  `review.source == Git { repository, commit, branch: None }` (`custom_nodes/setup.rs:21-31, 219-246`);
  `installed()` `:308-347` compares the same expected source. Default-branch mode already exists:
  `git.rs:383-432` `reference: None` → `ls-remote --symref HEAD` → `RefKind::DefaultHead`, branch `Some(name)`;
  `files.rs:518-527` adds the DETACHED warning only for Tag/Commit. UI: wizard `setup-wizard.tsx` (Manager off by
  default `:95-97`, help text `:732-736`, review text `:872`, `:894` "Curated selection is not execution consent…"),
  run progress `run-progress.tsx` (Manager consent `:144-174`, curated review `:186-204, :272-296`), App render prop
  `App.tsx:22-25`. Plans: `m6-approved-plan.md:322-333` ("Selection is not execution consent", "Never automatically
  acknowledge newly discovered warnings"); NFR-SEC-3 (`REQUIREMENTS.md:473`).
- **Upstream heads now (main, `git ls-remote --symref`):** PlagueKind HEAD is still `d58d006` (stays unavailable);
  rgthree default branch is now `main` @ `d7297b42` (catalog had `dev` @ `5f940ddf`, which was incompatible with
  ComfyUI 0.42.1); GGUF, KJNodes, WAS moved; 18 others unchanged.
- **Profiles (explorer):** `recommended` is a JSON flag per profile (`data/profiles/*.json:7`); exactly one per OS
  asserted by `profiles/tests.rs:75-95`; wizard preselects the first eligible recommended (`setup-wizard.tsx:173-176`).
  py312 = revision 2, torch 2.10.0 / torchvision 0.25.0 / **torchaudio 2.10.0**, triton-windows >=3.6,<3.7, six
  experimental accelerators all default-off, `calibrated: false`. No Linux py312 profile. The flag is not used by any
  install/rebuild/repair decision (revision-dependent paths use the recorded snapshot).
- **Extra model paths (explorer):** read-only parser `crates/infrastructure/src/comfy_runtime/extra_paths.rs`, port
  `ExtraModelPaths` (`application/src/comfy_runtime/ports.rs:56-59`), command `comfy_read_extra_model_paths`, card on
  the **ComfyUI page** (`ui/src/features/comfy-runtime/comfy-page.tsx:321-340`). The library re-reads the YAML on every
  scan (`models/library/roots.rs:122-216`) and scans/watches extra roots; downloads only target `ComfyUI/models`
  (`DEFAULT_MODEL_ROOT_ID`, `acquisition/stack.rs:30-31, 64-70`). ComfyUI loads `ComfyUI/extra_model_paths.yaml` by
  default (app passes no `--extra-model-paths-config`; users cannot either — reserved). ComfyUI's loader
  (`ComfyUI/utils/extra_config.py:6-34`): per top-level section, `base_path` → `expandvars(expanduser())`, relative
  → yaml dir; `is_default`; each other key = folder name, value split on `\n`, joined to base_path. Requirement
  FR-COMFY-8 (P1): "Read it, and optionally manage it". Atomic write helper `shared/fs.rs:6-23`.

## Proposed packages (sequential, one coder each; main verifies the full check list after each)

### Q — quick fixes (one package)
- **Q1 (P0)** add `"USERNAME"` to `USER_VARIABLES_WINDOWS` (layout.rs), with a one-line comment (Python 3.12
  `getpass.getuser()` falls back to `pwd`). Test `#[cfg(windows)] python_children_get_the_windows_user_name`
  (Tool::Python env contains USERNAME equal to the parent's when the parent has it). Keep
  `arbitrary_parent_variables_are_not_inherited` passing. Leave `system_baseline` and the process.rs test that asserts a
  baseline-only child has no USERNAME unchanged.
- **Q2** shell: `app-shell.tsx:108` → `relative flex h-screen overflow-hidden …`; `:110` column `min-h-0`;
  `:112` main → `relative min-h-0 flex-1 overflow-y-auto p-6` (positioned so absolutely positioned descendants are
  clipped and scroll inside main). No global body overflow change (SetupFrame relies on document scroll). Vitest:
  assert the shell root and main carry the containment classes. Main verifies natively (screenshot of Settings
  scrolled to bottom, no document scrollbar).
- **Q3** active row: replace the brand fill with a readable highlight in both tables:
  `bg-muted shadow-[inset_3px_0_0_var(--accent)]` (pink left bar) for the active row; keep zebra/hover; checked rows
  (`aria-selected`) get `bg-muted/60`. Contrast: dark #e0e5ec/#b8c0c9 on #3b4652 ≈ 8.5:1/5.9:1; light #2a313a/#56606c
  on #d2d8e0 ≈ 10:1/4.6:1. DESIGN.md Tables section: add "active row = `bg-muted` + 3px accent inset bar; never a
  brand fill behind table text".

### F — setup automation (one package)
- **F1 auto-consent in first-run setup.** `SetupRequest.third_party_consent: bool` (wire `thirdPartyConsent`, required
  on the wire like the other fields). Accepted only for `SetupMode::FirstRun`; any other mode with `true` → validation
  error (new code `third_party_consent_invalid`); Rebuild/Repair derivations force false. Persisted with the run's
  request document (serde default false — no migration if the request is stored as JSON; coder confirms).
  - Manager step: when `third_party_consent` and the resolved commit differs from `run.manager_consent`, set
    `run.manager_consent = Some(resolved.commit)`, persist, log "ComfyUI-Manager <url> at <commit> was approved on the
    setup review page." and continue (no pause).
  - Curated step: after `prepare_curated`, when `third_party_consent`, synthesize a decision for every Prepared plan not
    yet covered: `execute_node_code=true`, `acknowledge_warnings=true` (each warning is written to the setup log and
    the entry message), `run_install_py=false`, no remember, no source builds, no confirmed protected changes. A plan
    with protected changes cannot be approved → that entry becomes `Skipped` with the message "Not installed: its
    requirements would change protected packages (…)." (nothing was modified yet) and the run continues. Decisions are
    re-synthesized after a backend restart (flag is persisted), so Retry never pauses for consent.
  - Without the flag every existing pause/consent path is unchanged (Repair with Manager, M5 node installs).
  - UI wizard review page: when Manager or ≥1 curated node is selected, show the NFR-SEC-3 disclosure: "Installing
    ComfyUI-Manager and the selected custom nodes runs third-party code on this computer; a Python environment is not
    a sandbox. Setup installs the latest commit of each repository's default branch, never runs install.py scripts,
    and does not ask again. Clicking Install confirms this." followed by the list of sources (Manager URL + each
    selected curated repository URL). Install sends `thirdPartyConsent = installManager || curated.length > 0`.
    Remove the copy "Manager selected; consent still required", "Curated selection is not execution consent…" and the
    Manager help text about pausing; keep run-progress consent UI for runs without the flag.
- **F2 latest default-branch commit for curated nodes.** Catalog schema 2: entries `{ repository, unavailable? }`
  (drop `branch` and `commit`; provenance text updated). `CuratedNodeSpec` loses `commit`/`branch`.
  `SetupNodes::prepare` uses `reference: None` (default branch HEAD via `ls-remote --symref`); the review is accepted
  when `review.source` is Git with the entry's repository and a resolved commit (any branch); the reviewed commit is the
  one installed (staged checkout is frozen at prepare, so no TOCTOU). `installed()` recognizes an entry by repository
  (+ existing digest/dependency checks). The installed node tracks its branch (normal Update works later). DTO
  `CuratedNodeDto` drops `commit`/`branch`; the run progress shows the resolved commit when known. PlagueKind stays
  `unavailable`; reason text: "Its repository tracks Python cache files (__pycache__), which the app refuses to
  install." Tests updated (`curated/tests.rs`, application curated tests) + new
  `curated_setup_prepares_the_default_branch_head`.
- **F3 recommended profile.** `win-py312-torch2100-cu130.json` `recommended: true`, `win-py313-torch2141-cu130.json`
  `recommended: false`; Linux unchanged (no Linux py312 profile). No revision bump (the flag does not affect install
  content; rebuild/repair use recorded snapshots). Update `profiles/tests.rs:75-95`. UI fixture untouched.
- **Docs (F):** REQUIREMENTS NFR-SEC-3 (+ first-run exception), FR-SETUP-9 (latest default-branch commit), §2.4/§5.4
  (Windows recommended = py312/torch 2.10, owner decision 2026-10-20); `docs/DOCUMENTATION.md` setup sections;
  HANDOFF §3 decision rows. `m6-approved-plan.md` stays historical with a pointer.

### X — `extra_model_paths.yaml` generator (one package)
- ComfyUI page "Extra model paths" card gains: "Model base folder" (folder picker + text), buttons **Generate** and
  **Remove generated section**; the read-only list stays below. Copy: "The app writes an `extra_model_paths.yaml`
  section that points every model folder at this base folder and creates missing folders. ComfyUI uses it from its
  next start." When ComfyUI is running: "Restart ComfyUI to use the new paths."
- Backend: application use case `ManageExtraModelPaths` (generate/remove) behind a new port method on the existing
  infrastructure adapter (`YamlExtraModelPaths::generate(root, base) / remove(root)`); commands
  `comfy_generate_extra_model_paths { basePath }` and `comfy_remove_extra_model_paths`, manager-window ACL +
  permission TOMLs + bindings + ACL test cases. After a write, trigger a library refresh (existing rescan path).
- File semantics: app section key `vjumpkung_desktop`; generated layout `base_path: <abs>`, `is_default: true`, one
  key per folder name = the union of the §4.6 categories, `configs`, and every existing subfolder of the install's
  `ComfyUI/models` (validated as `SafeFileName`); value `<name>/`; for `text_encoders`/`diffusion_models` the legacy
  alias folder (`clip/`, `unet/`) is added as a second line only if it exists under the base. Missing primary folders
  are created under the base. Other top-level sections of an existing file are preserved by data (re-serialized;
  comments are lost), and the previous file is first copied to `extra_model_paths.yaml.bak-<UTC timestamp>` whenever
  it is not byte-identical to the last app-generated content (header line `# Managed by vjumpkung ComfyUI desktop`).
  Remove deletes only the app section (and the file if nothing else remains). Writes are atomic (`shared/fs.rs`).
- Validation (fail closed with messages): base must be absolute, a local drive path or UNC share, existing or
  creatable; must not contain `$`, `%` or start with `~` (ComfyUI expands them); must not equal or be inside the
  install's `ComfyUI/models`, `envs/`, `data/` or `tools/`; no reparse point on the existing prefix (reuse
  `reject_reparse`). YAML is emitted with `serde_yaml_ng` (correct quoting for spaces/Thai).
- Downloads keep targeting `ComfyUI/models` (unchanged; documented as a limitation) — see question Q-X1.
- Tests: generator (fresh, preserve other sections + backup, regenerate idempotent without new backup, remove,
  alias lines only when present, refusal cases incl. `$`/`%`/`~`/inside root), round trip through the existing parser
  (generated roots == expected), use case, ACL, UI (generate/remove flows, running hint).
- Docs: DOCUMENTATION model-library section, REQUIREMENTS FR-COMFY-8 note.

### G — git-friendly ComfyUI checkout (owner follow-up, 12:45: "make a ComfyUI clone before the program creates
folders, for making updating ComfyUI by git pull easily")
Evidence (explorer + main): owner checkout `D:\comfyui-test\v1\ComfyUI` is complete and clean but `HEAD (detached at
63be2f9b)` (tag v0.42.1); a local `master` exists (created by the plain `git clone`), `origin/HEAD -> origin/master`;
`git pull` → "You are not currently on a branch". Setup: `source.rs:286-326` full `git clone` into
`<root>/.ComfyUI.cloning`, then `checkout --detach <commit>` for Latest/Tag (`:312-316`; branch refs use
`checkout -B <b> --track origin/<b>` `:305-311`), then rename into an absent `ComfyUI` (`:270-276`) or
`publish_scaffold` merge (publish.rs) because startup composition already created `ComfyUI/models`
(`acquisition/stack.rs:64-70`, `library/roots.rs:61-80`, both pinned) and `custom_nodes/.disabled/.vjump-no-hooks`
(node reconcile `state.rs:591-598`, `paths.rs:88-108`). A moved root already composes in restricted mode without both
model stacks (`state.rs:871-878, 955-978`); a successful Rebuild already requests a shell restart
(`env_commands.rs:176-186, 207-211`); first runs do not recompose today. App updates: `comfy_runtime/git.rs` apply =
`merge --ff-only` for BranchFfOnly else `checkout --detach <commit>` (`:268-272`); the Version card shows
"Fast-forward {branch}" only when HEAD is on a branch with an upstream (`versions-panel.tsx:172-180`); policy
`domain/comfy_runtime/update.rs:67-90` (dirty, unfinished op, diverged; BranchFfOnly needs branch + upstream).
FR-COMFY-6: "git fetch then switch to the selected tag, or pull --ff-only on a branch … no reset or force".
- **G1 clone first.** At composition, when `<root>/ComfyUI/.git` does not exist (fresh root, nothing cloned yet), compose
  in a *pre-install* mode: do not open the download stack or the library stack and skip node reconcile/scan, so nothing
  is created under `ComfyUI/` (reuse the restricted-mode "model stacks absent" handling; commands that need them return
  the existing not-available error; the wizard does not use them). After a successful FirstRun activation, request the
  same shell restart as Rebuild so services are recomposed with the new checkout. The clone then goes through the
  existing "ComfyUI absent → rename stage into place" path; the scaffold-merge path stays for roots scaffolded by older
  builds. Tests: pre-install composition creates no `ComfyUI` entry; first-run success returns restart; existing restart
  handling in the UI.
- **G2 stay on a branch so `git pull` works.** One infrastructure helper `attach_or_detach(target_commit)` used by the
  setup clone (Latest/Tag) and by the app update apply for Tag/LatestRelease targets: determine the remote default
  branch `D` from `refs/remotes/origin/HEAD`; if the target commit is an ancestor of `origin/D` and local `D` either
  does not exist or has no commits that are not in `origin/D` (and the tree is clean — already enforced), run
  `git checkout -B D <commit>` and `git branch --set-upstream-to=origin/D D`; otherwise fall back to today's
  `checkout --detach <commit>` and log why. Result after setup: `On branch master … behind 'origin/master' by N
  commits, and can be fast-forwarded` → `git pull` fast-forwards; the Version card then also offers "Fast-forward
  master" (= pull --ff-only with requirements reinstall and the pin guard). Existing detached installs are attached on
  their next app update/switch. `install.json` `comfyui_reference` stays the tag (Latest/Tag) — unchanged semantics.
  Tests (real git, local bare repo fixtures like `source/tests.rs:775-827`): tag clone ends on `master` tracking
  `origin/master` at the tag commit and `git pull --ff-only` advances it; non-ancestor tag → detached fallback; local
  `master` with unique commits → detached fallback, commits preserved; update Switch-to-tag on an attached repo keeps
  the branch; update on a detached repo attaches.
- Docs: DOCUMENTATION ComfyUI updates section ("ComfyUI is a normal git checkout on `master`; `git pull` works; prefer
  the app's Fast-forward so requirements are reinstalled with the pin guard; after a manual pull run Repair"),
  REQUIREMENTS FR-COMFY-6 note.

### Then (unchanged, already advisor-approved in session 5)
D3+D5 (`prompts/session5/d3-copied-python-link.md`), D4 (`d4-hf-home.md`), D1 (`d1-r6-closure.md`) — prompts get the
new HEAD/working-copy line; then R (frozen binary, native pass incl. a fresh first run with the NEW recommended
profile + 21 curated at latest commits + auto-consent, Settings/Models/Custom Nodes screenshots), RELEASE_ACCEPTANCE,
README + DOCUMENTATION.

## Questions for the advisor (classify AI-decidable vs user-only; decide the former)
1. F1: is "disclosure on the review page + Install click = consent" an acceptable NFR-SEC-3 reconciliation, or must there
   be an explicit checkbox? (Owner explicitly asked for no permission prompts in first-time setup.)
2. F1: auto-acknowledging warnings for curated plans (contradicts m6 plan "never automatically acknowledge newly
   discovered warnings") — acceptable under the owner's instruction if every warning is logged and shown on the entry?
3. F1: protected-change plans → `Skipped` (vs fall back to the manual pause, vs Failed).
4. F1: scope = FirstRun only (Repair with Manager keeps its pause) — correct reading of "first time setup"?
5. F2: drop `branch` from the catalog (use the remote default HEAD) vs keep it as the requested branch.
6. F3: flip without revision bump; keep `calibrated:false` (owner install measured: env 4.25 GB, ComfyUI 0.36 GB,
   tools 0.13 GB, cache 0.25 GB after cleanup).
7. X: `is_default: true`? Should app downloads also go to the base folder when it is configured (scope growth:
   authorizing a second download root), or stay in `ComfyUI/models` for now (Q-X1)?
8. X: preserve-other-sections-with-backup vs refuse when a hand-written file exists.
9. Q2: is the `relative`/`min-h-0`/`overflow-hidden` containment the right fix without identifying the escaping element?
10. Package order and whether Q can run before advisor sign-off on F/X (it is independent). (Main already started Q
    with the Sonnet coder at ~12:50 because it is trivial and file-disjoint; flag anything wrong in Q.)
11. G1: is the pre-install composition + post-first-run restart the right mechanism, or is a lazy creation of the model
    roots on first use simpler/safer? Any composition pieces besides the two model stacks and node reconcile that write
    under `ComfyUI/`?
12. G2: is `checkout -B D <commit>` (moving a local branch pointer that has no unique commits) compatible with FR-COMFY-6
    "no reset or force"? Should switching to an OLDER tag on an attached repo detach instead (so `git pull` would not
    silently jump forward), or keep the branch (owner wants `git pull` to work)?
13. Package order proposal: Q → G → F → X → D3+D5 → D4 → D1 → R. G before F because both touch setup execution
    and the run/restart flow.

**GO for R with the corrections below, after I is verified and releases the build lock. NO-GO for declaring Release 1 ready.**

R can finish its audit and documentation with explicit open gates. Producing Windows bundles and documenting missing acceptance does not satisfy the approved Windows/Linux release scope. The plan explicitly distinguishes implementation completion from release readiness. [Approved plan:806](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:806), [approved plan:820](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:820).

I read the requested documents and relevant implementation/test definitions. I changed nothing and ran no builds or tests. I accept the supplied startup-fix results and machine specifications as main-verified evidence; I did not independently reproduce them.

**Decisions on Q1–Q4**

| Question | Decision | Classification and evidence |
|---|---|---|
| **Q1: default curated selection** | Run N1 with the actual default—all 22 selected, Manager off, py313 accelerators empty. Record any failure as a finding. Do not quietly reduce the selection and call that default acceptance. | **AI-decidable.** I’s approved prompt explicitly initializes all catalog entries selected. [I prompt:249](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/prompts/session5/i.md:249). |
| **Q1: recovery after curated failure** | Do not assume the whole step can always be skipped. Skip is refused while any curated entry is `Failed`. If needed, use a separate fresh root with curated nodes explicitly deselected to continue independent acceptance; preserve the failed default run. | **AI-decidable.** [setup.rs:383](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/environment/setup.rs:383). Residual #16’s “until skipped” wording needs qualification. |
| **Q2: debug + custom-protocol** | Accept it as native Windows integration evidence for setup, CUDA, embedded assets/CSP and rebuild. Label it **debug/custom-protocol, fresh install root on an existing development machine**. It does not establish release-binary, installer, production bootstrap or clean-machine acceptance. | **AI-decidable.** This build is explicitly in the approved checks. [Approved plan:799](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:799). The override exists only under `debug_assertions`. [paths.rs:169](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/paths.rs:169). |
| **Q2: root isolation** | Require the owner’s bootstrap and installation to remain unchanged. Never invoke Locate/New/root switching during override runs. Those commands refuse the debug override anyway. | **AI-decidable.** [switcher.rs:119](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/switcher.rs:119), [opener.rs:67](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/relocation/opener.rs:67). |
| **Q2: credential isolation** | A new root **does not isolate credentials**. Public HF requests still read and use the saved HF credential if present. Proceed without owner credentials only after establishing “not set,” or in a separate credential context. Never clear or replace the owner’s entries for testing. | **AI-decidable implementation finding; using existing credentials is user-only.** Keyring service/accounts are fixed, and HF metadata requests attach any returned credential. [secrets.rs:103](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/settings/secrets.rs:103), [huggingface.rs:61](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/models/acquisition/huggingface.rs:61). |
| **Q3: installer execution** | Do not run either installer on the owner’s host under the current plan. Use a disposable Windows VM/snapshot or another authorized test machine. Uninstalling afterward is insufficient rollback for installer/runtime/registration changes. | **User-only for changing the owner’s host.** Full install/launch/uninstall remains required acceptance. [Approved plan:736](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:736). |
| **Q4: release scope** | Linux compilation/native acceptance, unpassed provider requirements, installer/data-preservation acceptance and unresolved consequential security findings remain release gates. Machine limitations explain an open gate; they do not waive it. | **AI-decidable under existing scope.** [HANDOFF:69](/D:/vjump/vjump-comfyui-portable-with-resource-manager/HANDOFF.md:69), [approved plan:808](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:808). |
| **Q4: owner decisions** | Preserve unsigned bundles, inactive updater, placeholder icons and existing license status. None blocks performing R. Inactive updater does **not** make FR-SET-4 passed; signed-update acceptance remains open. Do not invent release waivers, keys, artwork or a license. | **Already owner-decided; no question to repeat.** [HANDOFF:80](/D:/vjump/vjump-comfyui-portable-with-resource-manager/HANDOFF.md:80), [IMPLEMENTATION_STATUS:76](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/IMPLEMENTATION_STATUS.md:76). |

The debug build also retains the production identifier. Single-instance routing and WebView2 storage are not scoped by the install-root override. Close any existing owner instance normally, then verify the acceptance process’s root before interacting with it. Do not claim complete Windows-profile isolation.

**Corrected R sequence**

All commands below are instructions for main later, after build ownership transfers.

1. **Freeze the verified I tree and establish evidence provenance.**

   Record HEAD, cumulative checkpoint hash, untracked source-file hashes, toolchain versions, machine facts and the exact build commands. HEAD alone is insufficient while the startup fix/I remain uncommitted.

   ```powershell
   git rev-parse HEAD
   git status --short
   rustc -Vv
   pnpm --version
   node --version
   nvidia-smi
   ```

   Keep raw evidence under `D:\vjc-s5\evidence\<evidence-id>\`; put sanitized summaries and links in the repository. Each run needs timestamps, binary SHA-256, root, profile/revision, selections, expected result, actual result and exit/result status.

   Reuse main’s completed I verification only when it covers this exact source tree. Obtain missing coverage separately:

   ```powershell
   cargo llvm-cov --locked -p domain -p application --json --output-path D:\vjc-s5\evidence\coverage.json
   ```

   Report domain and application percentages individually against ≥90%/≥80%. Historical coverage is insufficient. [Approved plan:804](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:804).

2. **Prepare the isolated native launch.**

   Build UI before embedding; do not rebuild if main already produced and identified these exact artifacts:

   ```powershell
   pnpm -C ui build
   cargo build -p desktop --locked --features tauri/custom-protocol
   Get-FileHash -Algorithm SHA256 -LiteralPath .\target\debug\vjumpkung-comfyui-desktop.exe
   ```

   Snapshot existence/hash of `%APPDATA%\vjumpkung-comfyui\bootstrap.json`, and establish the owner-root baseline. Do not create a missing bootstrap merely for evidence.

   Launch with a child-specific override:

   ```powershell
   $launchInfo = [System.Diagnostics.ProcessStartInfo]::new()
   $launchInfo.FileName = (Resolve-Path .\target\debug\vjumpkung-comfyui-desktop.exe).Path
   $launchInfo.UseShellExecute = $false
   $launchInfo.Environment['VJUMPKUNG_COMFYUI_HOME'] = 'D:\vjc-s5\a'
   $acceptanceProcess = [System.Diagnostics.Process]::Start($launchInfo)
   ```

   Confirm the visible root and PID before setup. After each run, check bootstrap/owner-root preservation.

3. **Run the native matrix below.**

| Run | Disposition here | Required observation |
|---|---|---|
| **N1: default first run** | **Mandatory** | Actual defaults, all 22 curated selections, one batch review, initially unchecked consent and separate warning acknowledgement, successful setup/activation, CUDA `/system_stats`, final import/CUDA verification, embedded window, Stop. Capture all curated outcomes and ComfyUI import errors—not just successful server readiness. |
| **N4/N5: public HF + crash** | **Mandatory once credential isolation is established; otherwise explicit blocker** | Pin repository commit/path, size and expected SHA-256. Show CLI/estimated progress, pause/restart-from-zero, resume completion and verified publication. For crash, kill only the recorded acceptance PID during an observed transfer; restart on the same root and capture paused state, staging cleanup and successful resume from zero. |
| **N6: unlocked rename** | **Mandatory** | Rename a disposable library fixture while real ComfyUI runs; check filesystem, catalog and sidecars. Add a case-only rename and a filename with Thai characters/spaces. This covers only part of S16. |
| **N7: Registry installation** | **Mandatory** | Choose a reviewed exact release with a harmless real dependency operation. Capture consent, `id@version`, dependency outcome and protected versions before/after. A dependency-free node alone does not establish the dependency clause of S10. |
| **N8: dirty Git + enable/disable** | **Add; cheap and mandatory** | On a disposable installed Git node, make a tracked text edit, request Update with an explicit target if detached, capture refusal/status and unchanged content; disable/enable and check the round trip. This is feasible here without tokens. |
| **N9: controlled Windows lock + trash** | **Add; cheap, partial S8** | Hold a non-delete-sharing handle on a disposable model file; exercise rename/delete refusal through the UI. Release the handle, retry, and confirm Recycle Bin placement. Label this a controlled sharing-lock check, not a loaded-ComfyUI-model check. |
| **N2a: moved root, original present** | **Mandatory after obtaining a healthy root** | Copy a stopped installation, detect `rebuild_required`, rebuild/restart, start CUDA ComfyUI at the destination, preserve model/node/user data, and prove original-root preservation. |
| **N2b: original absent** | **Required to close the plan’s relocation gate; otherwise open** | Use another disposable copy and park its disposable predecessor before launch. Rebuild and start without access to the recorded old path. Do not disturb the owner’s installation. |
| **N3: owner’s failed run** | **Keep open; do not perform as proposed** | A relocated copy is not “Retry same run.” Retry checks the recorded root against the current root and refuses a mismatch. Testing the actual owner run needs separate authorization and a protected backup. [setup.rs:459](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/environment/setup.rs:459). |

For N5:

```powershell
taskkill.exe /PID $acceptanceProcess.Id /F
```

Do not kill by image name. After rebuild restarts, rediscover and verify the new acceptance PID.

For N2a, stop ComfyUI and close the manager completely before copying:

```powershell
robocopy.exe D:\vjc-s5\a D:\vjc-s5\b /E /COPY:DAT /DCOPY:DAT /R:0 /W:0 /XJ /LOG:D:\vjc-s5\evidence\N2a-copy.log
if ($LASTEXITCODE -ge 8) { throw 'Acceptance copy failed' }
```

Preflight reparse points: `/XJ` must not silently omit something and still yield a claimed complete copy.

Generate sorted manifests containing relative paths, entry types, file sizes and SHA-256 values. Include hidden files, `data/app.db`, any WAL/SHM files, tools and environments. Compare `a` immediately before launching `b` and after `b` closes; perform no other operations on `a` during that interval. Capture DB evidence from copies afterward so inspection cannot perturb the preservation baseline.

For CUDA evidence, use the actual UI-selected port:

```powershell
curl.exe --fail --silent --show-error "http://127.0.0.1:$comfyPort/system_stats" --output D:\vjc-s5\evidence\N1-system-stats.json
```

Retain the app’s final verification output: it already imports `torch`, `triton`, `comfy` and performs a CUDA tensor operation. [verify.rs:30](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/verify.rs:30).

Additional opportunities and limits:

- **S6:** public Xet, gated-without-token error and missing-CLI fallback are feasible once credentials are isolated. Rename the HF executable only in a disposable root. Ordinary HF success does not prove Xet relay enforcement.
- **S7:** public Google Drive confirmation needs a suitable public fixture, **not a Drive token**. Try it if a fixture is available; quota handling can remain open.
- **S2:** native picker selection/cancellation and validation are cheap; complete empty/adopted-venv setup remains optional additional work here.
- **S3:** the same-Python cu130→cu126 replacement is feasible but another substantial setup. Leave it explicitly open if omitted.
- **S14 node crash / S16 gate race:** attempt only during an observed relevant phase. Killing at a review dialog or clicking Start after installation finishes proves neither.
- **S17:** an impossible requirement rejected during preview does not exercise failed mutation and Repair. Keep native S17 open unless a controlled post-consent failure is actually exercised.
- **S9/S15:** suitable current automated evidence can establish deterministic cases. Preserve the owner-disabled-CI exception; do not enable workflows.
- **S13 and Linux-specific acceptance:** remain open. Do not disable the owner’s GPU/change drivers to manufacture unsupported hardware.

4. **Complete the missing runtime/UX audit.**

   Add bounded checks while the app is available:

   - Offline manager rendering, both themes, Thai filenames, keyboard/focus and 200% zoom.
   - Real xterm geometry/progress rendering.
   - Headless close/reopen and process cleanup with real ComfyUI.
   - Embedded-window ACL evidence; distinguish compiled-policy tests from native invocation evidence.
   - Combined native/HF live bandwidth cap and cancellation while throttled, if N4 is available.
   - Cached 2,000-file scan timing and uncapped transfer versus the same URL using curl.

   Anything omitted remains open under the relevant NFR/plan row. These checks are explicitly part of the approved minimum evidence. [Approved plan:812](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/handoff/plans/m6-approved-plan.md:812).

5. **Build and inspect Windows release bundles; leave installation acceptance open.**

   In a build-only shell with updater/signing configuration unset:

   ```powershell
   pnpm -C ui tauri build
   ```

   Capture the complete log and exit code. Enumerate the actual outputs:

   ```powershell
   Get-ChildItem -LiteralPath .\target\release\bundle -Recurse -File |
     Where-Object { $_.Extension -in '.exe', '.msi' } |
     ForEach-Object {
       [pscustomobject]@{
         File = $_.FullName
         Bytes = $_.Length
         SHA256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
       }
     }

   (Get-Item -LiteralPath .\target\release\vjumpkung-comfyui-desktop.exe).VersionInfo |
     Select-Object FileVersion, ProductVersion

   Get-AuthenticodeSignature -LiteralPath .\target\release\vjumpkung-comfyui-desktop.exe |
     Select-Object Status, StatusMessage
   ```

   Inspect generated WiX/NSIS definitions and artifact metadata for:

   - Actual discovery of the Windows overlay.
   - Matching Cargo/Tauri/npm/executable/bundle versions—currently configured as `0.1.0`.
   - NSIS `currentUser`, MSI installation scope, WebView2 bootstrapper configuration.
   - Included files, shortcuts and uninstall deletion scope.
   - Identifier, CSP, capabilities, fonts, placeholder icons, license status and third-party notices.

   The wrapper supplies the app path and delegates overlay discovery to Tauri; it does not explicitly merge overlays itself. [tauri.mjs:23](/D:/vjump/vjump-comfyui-portable-with-resource-manager/scripts/tauri.mjs:23). Configuration inspection cannot prove bootstrapper execution or uninstall data preservation.

   Do not launch the release executable on the owner’s account merely to smoke-test it: the debug root override is ignored there. Later installer acceptance should use the exact hashed artifacts in a disposable OS, including retained installation data across uninstall/reinstall.

6. **Write the ledger, then derive the final documentation from it.**

**Q5: `docs/RELEASE_ACCEPTANCE.md` structure**

Use these sections/tables:

| Table | Required columns |
|---|---|
| Snapshot | Source checkpoint, binary/artifact hashes, date, OS/toolchain/GPU/driver, configuration, isolation, limitations |
| Requirements | Requirement ID, normative priority, clause/platform/profile, automated evidence, native evidence, status, remaining gap, release gate, owner/next action |
| Scenarios | S01–S17, subclause, Windows result, Linux result, evidence IDs, remaining blocker/owner |
| Evidence register | Evidence ID, command/UI procedure, inputs, expected/actual result, timestamp, exit status, artifact paths/hashes |
| Residuals | Existing R01–R21, current evidence, disposition, consequence/workaround, owner |
| Release gates | Gate ID, unmet criterion, required closure evidence, responsible owner |

Evidence IDs should be stable, for example:

- `S5-AUTO-WIN-001`
- `S5-NATIVE-WIN-N1-001`
- `S5-PKG-WIN-001`
- `S5-OPEN-LINUX-001`

Use `PASS`, `PARTIAL`, `FAIL`, `OPEN`, `ACCEPTED RESIDUAL`, and `OWNER DECISION`. An open blocker or accepted residual is never a passing test. Split compound scenarios so N4 cannot turn all of S6 green.

Include the unprioritized NFRs and §6’s reliability/platform/performance/observability clauses; do not silently exclude them because they lack P0/P1 tags. Split FR-NODE-6’s P1 “Update all” clause from its P0 operations.

Refresh the ignored-test inventory: there are now **10 definitions**, including native `startup_smoke`. Separate explicitly executed ignored tests, unexecuted acceptance tests, the binding exporter and helper subprocess fixtures. The explorer’s nine-test inventory predates the startup test.

README/DOCUMENTATION must:

- State the actual release verdict and link the ledger.
- Distinguish implemented behavior, automated coverage and native acceptance.
- Name verified OS/profile/hardware/build/date precisely.
- Keep Linux compilation and native support unverified until evidence exists.
- Explain inactive updater, unsigned installers and experimental accelerators accurately.
- Preserve relevant residuals and recovery limitations.
- Replace every VERIFY marker with verified wording or an explicit limitation—not simply delete it.

Correct the outdated “no NVIDIA” statements in HANDOFF, status and documentation. Preserve the historical 2026-10-16 direct-test evidence as historical evidence.

**Risks requiring explicit disposition**

- **Residual #6 remains an open hardening gate.** The comment claims child protection, but the helper permits `ERROR_BAD_EXE_FORMAT` before initializing its own policy. Passing that test does not establish protection for uv/MinGit/Python children. [windows.rs:27](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/windows.rs:27), [tests.rs:693](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/tests.rs:693). The approved H plan required exclusion in the child. Record the mitigation gap; investigate actual pinned-tool exposure or implement an effective reviewed correction. This is not evidence of a demonstrated exploit, and it is not already an accepted residual.
- **Residual #21 is not closed by N2.** A valid-manifest copy/rebuild does not establish safety for absent/corrupt manifests with old-root DB paths. Current restricted composition depends on a valid moved manifest. [state.rs:867](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/desktop/src/state.rs:867). Keep this separately gated pending containment evidence/correction.
- **Empty py313 accelerator selection provides no wheel compatibility evidence.** Residual #19 remains open. A successful default setup cannot validate experimental py312 wheels.
- **N2a alone misses the explicitly required original-absent case.**
- **Unsigned/inactive/placeholder status is already authorized; missing functional evidence is not a release waiver.**
- Any corrective code work discovered during R requires a new identified source checkpoint and relevant re-verification before dependent evidence is accepted.

**Success checks**

R’s audit is complete when every P0/P1 requirement, applicable NFR/clause, all 17 scenarios and all 21 residuals have a supported disposition; available mandatory runs have evidence or a concrete blocker; Windows artifacts are identified and audited; documentation agrees with the ledger; and no owner installation/credential was altered or used without authorization.

Release readiness remains **NO-GO** while mandatory release gates are open.

No owner answer is needed to begin this corrected audit. Only these conditional questions are indispensable for closing gates:

1. **If an HF/CivitAI credential is already saved:** may acceptance use it? **Recommendation:** use a separate test credential context; otherwise leave authenticated provider acceptance open.
2. **Where may installer acceptance run?** **Recommendation:** a disposable Windows VM/snapshot, plus a native Linux/NVIDIA test machine. If neither is available, retain those release gates rather than install on the owner’s host.
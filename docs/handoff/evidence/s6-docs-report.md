# Session 6 docs package report

Date: 2026-10-20. Prompt: `docs/handoff/prompts/session6/docs-final.md`. Source facts: the code at checkpoint
`s6-14-final-code-verified` (working copy), `s6-packaging.md` (final snapshot), `s6-native-register.md` (N1–N29),
`s5-native-register.md`, `s3-residuals.md` (R01–R28), `s5-advisor-r.md` (Q5 structure), the session-6 package
reports and rulings. No code, test, binding or configuration file was touched; no git write command was run; no
build was run. Read-only commands only (rg/grep/sed, a Python link checker in the session scratchpad).

## Files written

| File | Summary |
|---|---|
| `docs/RELEASE_ACCEPTANCE.md` (new) | Q5 structure: snapshot (checkpoint + SHA-256, artifact hashes, debug binary hashes, machine, toolchain, configuration, isolation, limitations); requirements table for every P0/P1 FR (FR-NODE-6 split into P0 operations and P1 Update all) and every NFR clause, including ledger IDs for the unnumbered §6.2–§6.4/§6.7 bullets; scenarios S01–S17 split into subclauses with Windows/Linux results; evidence register (S6-AUTO-WIN-001…012, S6-PKG-WIN-001…004, S5-AUTO-WIN-001, HIST-WIN-2026-10-16, S6-NATIVE-WIN-N1…N29, S5 native IDs, S6-OPEN-*); ignored-test inventory; residuals R01–R28; release gates G01–G10. Verdict **NO-GO**. |
| `README.md` (rewrite) | Tauri 2 + React (not Electron); NO-GO status linked to the ledger; session-6 features; Release 1 scope table with corrected Defaults row (Windows py312/torch 2.10 cu130, Linux py313/torch 2.14.1 cu130, ComfyUI source choices) and corrected disk budget (17 GiB for the py312 profile); verified hardware/OS/date; user and developer quick start with the AGENTS.md/§5.6 commands plus the smoke test; layout; documentation links; security notes (D1 refusal text, NFR-SEC-3 first-run exception, token handling); updater/signing; license (owner decision). The one VERIFY marker is resolved. |
| `docs/DOCUMENTATION.md` (update in place) | Structure and TOC kept. Status-marker section rewritten (native vs automated vs Linux, verdict, ledger link). All 24 VERIFY markers resolved. Updated: installation (built, unsigned, not run), wizard (profiles in recommended order, ComfyUI source incl. Latest commit (master), options defaults, accelerators, exact review disclosure, no consent pauses, restart after first run), re-setup node requirements, moved install (screens, codes, copied uv links, `--no-registry`, R21), Environment page accelerator card, ComfyUI page (lease, error texts, branch tracking evidence, extra model paths evidence, diagnostics), downloads (hf private home, N23/N24, bandwidth cap and relay), library evidence, custom nodes (D1 boundary, install.py label, protected packages, Update all groups, N29, recovery, Resume review, Manager Observed limitation), settings (bandwidth, App updates, Diagnostics), troubleshooting (node/rebuild/root codes, close refused during rescan, Python 3.12 USERNAME, debug stack overflow, WebView2 relaunch), data paths (`hf/home`, `wheels/<sha256>`, `backups`), developer sections (check list + final counts, smoke test, commands, migrations 007/008, profile schema 2, opt-in tests incl. `VJ_REAL_HF_*`, packaging result, updater design), security model (Git safety, native-image boundary, child processes, pinned containment, Git content identity), known limitations rewritten. |
| `docs/IMPLEMENTATION_STATUS.md` | Header moved to the final checkpoint and N1–N29; M1/M4/M5/M6/Docs rows updated (D1 + N/D1b done, packaging done); verification block extended to 802/0/12 and Vitest 30/202 with the final snapshot and ignored-test summary; release gates refreshed (D1, packaging build and ledger closed). |
| `docs/REQUIREMENTS.md` | §9 only: resolved (Xet via `hf` natively verified N23/N24; Windows default stack verified) and open items (Linux, accelerator wheels incl. unvalidated py312 wheels and FA3 module name, native-engine Xet, `hf` resume, WebKitGTK fonts/canvas, signing/key custody, provider tokens, installer execution). Normative rows untouched. |
| `docs/handoff/evidence/s6-docs-report.md` | This report. |

All six files use LF line endings and end with a newline (checked byte-wise).

## VERIFY markers

| File | Before | After |
|---|---:|---:|
| `docs/DOCUMENTATION.md` | 24 | 0 |
| `README.md` | 1 | 0 |

The prompt and the status ledger said "26+" markers in DOCUMENTATION; the file contained 24 VERIFY HTML comments at the start of this package. Each was replaced by verified wording (checked against the code) or an explicit limitation; none was simply deleted.

## Statements that could not be fully verified, and how they are phrased

| Topic | Phrasing used |
|---|---|
| Native-image refusal on the Custom Nodes page | Stated as covered by Windows real-Git tests; "no public repository with a top-level `.dll` was available for a native run" (as in the register). |
| Diagnostics copy, App updates panel, accelerator install, registry install, update/enable/disable/uninstall, re-setup, pause/resume, missing-tool fallback, crash recovery | Described from the code and labelled "covered by automated tests only" / "not run natively". |
| S06b (LFS verified via `X-Linked-ETag`) | PARTIAL: N24 verified a public file with a published SHA-256; a non-Xet LFS file was not separately exercised. |
| S01 clean setup | PARTIAL for Windows: fresh roots of the development machine with debug/custom-protocol builds, not a clean machine or an installed release. |
| Binary hashes of the debug acceptance binaries | Quoted truncated as in the native register; the frozen binaries under `D:\vjc-s6\` were not read. |
| Credential isolation in session 6 | Stated as "no provider token was used in any run; all Hugging Face runs used public files", without claiming the keyring was empty. |
| Coverage, performance, accessibility | Stated as not measured (OPEN), no numbers given. |
| Ignored-test count | The prompt said 11; the code has 12 `#[ignore]` definitions and the final run reports 12 ignored. The ledger uses 12 and notes the discrepancy. |
| `Create new install` | "the application restarts using it, and an empty root opens the setup wizard" (restart is in the command; the wizard follows from the empty root). |

## Link check

A Python checker (session scratchpad, run with `python -I`) followed every relative Markdown link and `#anchor` in
the five edited documents (code blocks excluded): **117 links checked, 0 missing targets, 0 broken anchors.**
External `https://` links were not fetched.

# Session 5 — advisor stand-in rulings D1–D5 (2026-10-20)

Codex hit its usage limit at ~11:00 (+07:00; reset stated 14:36). Per the handoff's tested fallback, a read-only
Opus agent stood in for the advisor; a separate read-only Opus agent acted as thinker for R6. All rulings are
AI-decidable; the full specifications are in the coder prompts under `docs/handoff/prompts/session5/`.

| ID | Finding (native, this machine) | Ruling | Package |
|---|---|---|---|
| D1 | Residual #6: before consent, uv preview runs with cwd = staged node dir and Git `-C <staging>` runs after checkout; the manager's `SetDllDirectoryW("")` is process-local (Codex advisor: release blocker vs NFR-SEC-3, `s5-advisor-r6.md`). | Option B accepted (replaces the Codex invariant): on Windows, refuse new node content whose top level has a native-image entry (dll exe com cpl drv ocx scr sys; Unicode uppercase) — registry after extraction, Git from `ls-tree` before checkout and again after; `Failed` (cleans staging). Children without an explicit cwd start in the executable's folder (launch-folder Python/Git risk). Live test that uv's interpreter query imports nothing from the node folder (stop condition if it does). Option A (trusted cwd + requirements rewriter) rejected: Git uncovered, contradicts F9. | `d1-r6-closure.md` |
| D2 | Default first run (all 22 curated) always fails: `ComfyUI-PlagueKind-Nodes@d58d006` (= upstream HEAD) tracks `bundle_nodes/__pycache__/*.pyc`, refused by `validate_fresh_git`; Retry re-fails; Skip skips every not-installed curated node. | O2 narrowed: keep the entry, mark it `unavailable` (reason shown, unselectable, never sent, backend refuses it). O1 (sparse checkout) rejected on merit; O3 (per-entry skip) deferred. Owner question (non-blocking): keep listed as unavailable (default) / remove / ask upstream to untrack the cache and bump the pin. | `d2-curated-unavailable.md` |
| D3 | Copied install (robocopy /E or Explorer) turns uv's minor-version link `cpython-3.13-windows-x86_64-none` (a junction) into a real directory → `uv python install` fails with os error 145 → Rebuild (and any setup on a copied root) fails at `tools`. | In `ensure_managed_python`: derive the link name from pinned metadata; leave links/reparse points to uv; remove only a real copied directory with the pinned non-following `remove_owned_tree`; fail closed otherwise. | `d3-copied-python-link.md` |
| D4 | Every `hf` CLI download fails on Windows: the child env (system baseline) has no `USERPROFILE`, hf 2.2.0 calls `Path.home()` at import → RuntimeError; cause never logged. | Give the hf child an app-owned home (`USERPROFILE`/`HOME` = `data/cache/hf/home`, never the real profile); log a bounded, redacted stderr summary on `ToolRuntime`. | `d4-hf-home.md` |
| D5 | `uv python install` without `--no-registry` registers each managed Python under `HKCU\Software\Python\Astral` (verified: CPython3.13.16 → `D:\vjc-s5\c\…`, CPython3.12.15 → `D:\vjump\test-comfyui\…`). | Always pass `--no-registry`; the app performs no registry cleanup (keys are shared/unprovable ownership); docs give the manual `reg delete`. Folded into the D3 package. | `d3-copied-python-link.md` |

Order: D2 → D3+D5 → D4 → D1, then one native pass (scenario 1 default with 21 curated, scenario 4 rebuild of a
copied root incl. no HKCU write, scenario 6 hf download, R6 live evidence).

Further notes from the thinker/advisor (recorded, not decided): ComfyRequirements in setup installs pre-existing
scanned nodes' requirements without per-node consent (wheel-only because the UI always sends
`allowSourceBuilds=false`; IPC still accepts `true` — latent). Curated import failures under the default profile
(native N1b): ComfyUI-MelBandRoFormer and ComfyUI-WanVideoWrapper need torchaudio (omitted by the torch 2.14.1
profiles by design); rgthree-comfy @5f940ddfbb83 is incompatible with ComfyUI 0.42.1
(`execution.recursive_output_delete_if_changed` missing) — catalog-maintenance findings for the owner.

# HANDOFF — vjumpkung-rust-comfyui-desktop

Last session: 2026-10-15. State: **requirements phase complete (spec v0.3). No code written. Nothing committed** — the repo has no commits, and `docs/` and `HANDOFF.md` are untracked.

## 1. What this project is
A Rust desktop app (Tauri 2 + system webview, not Electron) that:
- installs, runs and maintains a local ComfyUI;
- downloads and manages models from CivitAI, Hugging Face, Google Drive and direct URLs;
- manages custom nodes through the Comfy Registry or git;
- manages the Python, venv and torch builds, with a first-run wizard and re-setup.

## 2. Read these first (in order)
1. `docs/REQUIREMENTS.md`: the normative spec (FR/NFR IDs, priorities, architecture §3.3, acceptance scenarios §8, open items §9).
2. `docs/adr/0001-ui-technology.md`: why Vite + React (not native Rust UI or Leptos/Dioxus).
3. `docs/adr/0002-clean-architecture-ddd.md`: the Rust adaptation of Clean Architecture + DDD.
4. `docs/DESIGN.md`: the UI design system, adapted from vjump.dev.

Reference material, read-only and outside this repo:

| Path | Use |
|---|---|
| `D:\vjump\vjumpkung-sd-ui-manager-backend` | Python/FastAPI reference for downloads (CivitAI file selection, HF URL parsing, `hf download` preference) and custom-node install flow |
| `D:\vjump\vjump-resource-manager-frontend` | Next.js reference UI. Port its logic and structure, and restyle it per DESIGN.md. |
| `d:\vjump\pixellatent-happy-meals-scripts\comfyui-python-3-12-happy-meals\portable_version\install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat` (+ `update_comfyui_portable.bat`) | Setup steps, env isolation vars, accelerator wheels, verification, ff-only update safety |
| `d:\vjump\vjump-dev-new-website\DESIGN.md` | Upstream design system (fingerprint sha256 `36a7e467c0c6ba12…`) |

## 3. Decisions already made — do not re-litigate
| Decision | Who | Where |
|---|---|---|
| Release 1 = **Windows x64 + Linux x64, NVIDIA only** | User | §1.4 |
| Moved install → **guided rebuild** of the venv (keep user data); the install is not portable as-is | User | FR-SETUP-12 |
| **Tauri 2.12.x** (not v3 alpha, not raw wry/tao) | Advisor | §2.1 |
| **Vite + React + TS** frontend (Options A and B rejected) | Advisor, user asked | §2.2, ADR-0001 |
| **tauri-specta 2.0.0-rc.25** pinned for IPC bindings; fallback ts-rs 12 | Advisor | §2.1 |
| **Clean Architecture, Rust adaptation:** concrete use-case structs with `execute()`; traits only for DIP/streaming ports | User | §3.3, ADR-0002 |
| Crates per layer: `domain` ← `application` ← `infrastructure` ← `desktop`, plus `ui/`. Contexts are modules inside each crate. | Advisor | §3.3.2 |
| Contexts: `environment`, `comfy_runtime`, `models{acquisition,library}`, `custom_nodes`, `settings`, `shared` | Advisor | §3.3.1 |
| No serde in domain/application. `async-trait` for dyn ports. Errors per layer (`thiserror`). | Advisor | §3.3.2 |
| One **maintenance gate**, no event bus. Model rename/delete allowed while ComfyUI runs. | Advisor | §3.3.3–3.3.4 |
| Failed in-place pip / `install.py` → env **`repair_required`**, no venv snapshot/rollback | User | §6.2, FR-NODE-4 |
| Python **3.13** + torch **2.14.1 cu130** default; 3.12/torch 2.10 and cu126 profiles | Advisor | §2.4, §5.4 |
| uv-managed Python (not the embeddable zip). MinGit subprocess on Windows, system git on Linux. | Advisor | §2.3 |
| Tokens in the OS keyring only; never in URLs, logs or the DB | Advisor | FR-SET-1, NFR-SEC-1/2 |
| Delete → Recycle Bin/trash; never a silent permanent delete | Advisor | FR-MODEL-3 |
| ComfyUI-Manager optional, off by default | Advisor | FR-NODE-10 |
| **HF downloads default to the `hf` CLI** (2.2.0, isolated hash-locked tool venv). **Pause/resume restarts from 0** for that engine. Native engine is a setting, and a fallback only on tool launch failure. | User | FR-DL-3/3a/3b |
| Design system = **vjump.dev**, adapted (`radix-vega`, lucide, Inter/Geist Mono + Noto Sans Thai, bundled) | User | DESIGN.md, NFR-UX |
| UI copy **English only** for release 1 (Thai P2) | User | NFR-UX-2 |

## 4. Open items (spec §9 — need runtime validation)
- Accelerator wheels and their SHA-256 for py3.13 / torch 2.14.1 on both OSes: SageAttention, FlashAttention 2/3, onnxruntime-gpu, llama-cpp-python.
- Native HF engine vs Xet-backed files.
- Whether a newer `hf` restores cross-run resume (if so, revisit FR-DL-3a).
- WebKitGTK performance for the ComfyUI canvas; Thai font rendering on Linux.
- tauri-specta rc.25 compatibility with Tauri 2.12 (validate at M0).
- Windows code signing and updater key custody.

## 5. Suggested next step: M0 skeleton (spec §7)
1. Cargo workspace `crates/{domain,application,infrastructure,desktop}` plus `ui/` (Vite 8, React 19, Tailwind 4, shadcn `radix-vega`, lucide), with DESIGN.md tokens in `ui/src/app/globals.css`.
2. Tauri 2 shell with two webviews: manager (capability-scoped commands) and ComfyUI (no capability). Add a test that `invoke` from the ComfyUI webview is denied.
3. tauri-specta bindings checked in, with a CI drift check.
4. SQLite (rusqlite bundled) with migrations; settings; keyring secret store; `tracing` with redaction.
5. CI on Windows and Linux:
   - `cargo fmt --check`;
   - `clippy -D warnings`;
   - tests;
   - dependency-graph allow-list check (§3.3.5);
   - `eslint-plugin-boundaries`.
6. Then M1 Setup (environment context), and so on.

Verify current crate and npm versions before pinning. The versions in the spec were observed on 2026-10-15.

## 6. Working agreement (from the user's global CLAUDE.md)
- Main orchestrates: explorer (Codex `gpt-6-luna` xhigh, read-only) → plan → advisor (Codex `gpt-6.1-sol` xhigh, read-only) → coder (Codex `gpt-6.1-sol` xhigh, write) → verify.
- Call Codex via `/codex:rescue` with explicit `--model`, `--effort`, `--fresh`, scope and success criteria. Fallbacks: Haiku 5.5 (explore), Sonnet 5.5 (code).
- Ask the user only about decisions the advisor classifies as user-only.
- Background Codex jobs: check with `node C:/Users/vjump/.claude/plugins/cache/openai-codex/codex/1.0.6/scripts/codex-companion.mjs status`, then `result <task-id>`.
- Local clean-architecture plugin: `C:\Users\vjump\.claude\plugins\cache\anthropic-plugin-directory\clean-architecture\1.0.0-5a68049e6455\CLAUDE.md`. Its per-use-case port rule is intentionally adapted (ADR-0002).
- Commit only when the user asks. Branch off `main` first.

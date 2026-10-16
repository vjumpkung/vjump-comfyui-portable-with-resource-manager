# HANDOFF — vjumpkung-rust-comfyui-desktop

Last updated: 2026-10-16. **M0–M4 implemented and checked on Windows; M5 interrupted and not yet verified; M6 incomplete; README.md absent.** Branch: `feat/m0-skeleton`. Current HEAD: `d85b4a8` (`initial commit`), also referenced by `origin/feat/m0-skeleton`. The worktree was clean when this handoff was updated; this does not establish that the interrupted M5 implementation is complete.

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
- tauri-specta rc.25 compatibility with pinned Tauri 2.12.4 was validated at M0; retain the binding drift check.
- Windows code signing and updater key custody.

## 5. Implementation status and next steps

The active objective is to finish every milestone through M6, then write `README.md` documenting the program. Do not treat earlier green checks as proof that later edits pass.

| Milestone | Status / evidence |
|---|---|
| M0 Skeleton | Workspace, settings, SQLite, credentials, redacted logging, typed IPC, capability isolation and CI implemented. |
| M1 Setup | Setup backend, wizard and Environment page implemented. Earlier session reported a real RTX 3060 setup with Python 3.13.16 / torch 2.14.1 cu130, successful CUDA verification and healthy Repair. UI handoff: `ui/M1-UI-HANDOFF.md`. |
| M2 ComfyUI runtime | Runtime supervisor, launch/update/close flows, desktop commands and UI implemented. Earlier Windows checks passed; Linux and full native close/navigation acceptance remain unproven. |
| M3 Downloads | Provider adapters, hf/native engines, queue, durable finalization, commands and UI implemented. Late overwrite decisions now echo the observed file identity and reject stale decisions. Real provider acceptance remains outstanding. |
| M4 Library | Scanning/watching, model operations, identification, commands and UI implemented. Last main-agent check reported 369 Rust tests and 73 UI tests passing, clean fmt/clippy/architecture/lint/typecheck and UI build. These checks predate M5. |
| M5 Custom nodes | Partial implementation exists; coder stopped on a usage limit. Domain/application/infrastructure modules, migration 006, desktop node DTOs/commands and state composition are present. No `ui/src/features/custom-nodes/` exists. No completed verification report. |
| M6 Release 1 | Advisor plan approved with corrections. CI/release workflow files exist, but their presence is not completion evidence. Accelerators, curated nodes, moved-install rebuild, updater, global bandwidth and diagnostics need implementation/audit. |
| README | Does not exist. Write after the implementation audit, documenting actual behavior and remaining limitations. |

### Resume here
1. Inspect the interrupted M5 diff/current code. The stopped coder's last message was “Now state.rs modifications”; current `crates/desktop/src/state.rs` already contains `NodeServices`, recovery and composition wiring. Establish what still needs integration rather than assuming that message is the exact stopping point.
2. Finish M5, including Custom Nodes UI, gateway, generated bindings, production command permissions and valid-body ACL coverage. Recheck recovery, consent binding, protected pins and maintenance-gate behavior against the approved plan.
3. Run the checks below; fix failures without suppressing warnings. Finish M6 against its approved plan, then audit every requirement and write README.
4. Preserve fixed decisions in section 3. Do not claim Linux, real-provider, native recycling, scale/performance or release acceptance without evidence from those environments.

### Saved plans and advisor corrections
The previous session saved plans outside the repo at:

`C:/Users/vjump/AppData/Local/Temp/claude/D--vjump-vjump-comfyui-portable-with-resource-manager/d0238eb2-6d30-4e01-a5ad-f9ad8b7880d0/scratchpad/`

Read each plan together with its advisor corrections: `M1-PLAN.md` / `M1-ADVISOR.md`, `M2-PLAN.md` / `M2-ADVISOR.md`, `M3-PLAN.md` / `M3-ADVISOR.md`, `M4-M5-PLAN.md` / `M45-ADVISOR.md`, and `M6-PLAN.md` / `M6-ADVISOR.md`. These temporary files may disappear; normative repo docs remain authoritative.

M6 corrections include hashed exact accelerator wheels; one shared bandwidth bucket for native transfers and an allow-listed loopback CONNECT proxy for hf; side-by-side moved-install rebuilding; curated nodes through M5 consent; optional build-time updater configuration; redacted diagnostics; packaging and release checks. Updater key custody, Authenticode and project license remain owner decisions. Keep updater inactive without configuration, placeholder icons, and no invented license grant.

### Verification commands
Build the real UI before desktop production embedding. Ordinary tests must not rewrite checked-in bindings.

```powershell
pnpm -C ui install --frozen-lockfile
pnpm -C ui lint
pnpm -C ui typecheck
pnpm -C ui test
pnpm -C ui build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
node --test scripts/check-arch-deps.test.mjs
node scripts/check-arch-deps.mjs
cargo test -p desktop --test bindings --locked
cargo build -p desktop --locked --features tauri/custom-protocol
```

Also retain coverage targets (domain 90%, application 80%), mutation checks for architecture/ACL/binding drift, and the manual acceptance scenarios in REQUIREMENTS and advisor reviews. Last reported coverage was measured before later milestones and must be remeasured.

## 6. Working agreement and historical tooling
- Follow current AGENTS.md and runtime tools. The prior CLI workflow was: explorer (Codex `gpt-6-luna` xhigh, read-only) → plan → advisor (Codex `gpt-6.1-sol` xhigh, read-only) → coder (Codex `gpt-6.1-sol` xhigh, write) → verify.
- Historical Claude CLI routing (use only if available in the current runtime): call Codex via `/codex:rescue` with explicit `--model`, `--effort`, `--fresh`, scope and success criteria. Fallbacks: Haiku 5.5 (explore), Sonnet 5.5 (code).
- Ask the user only about decisions the advisor classifies as user-only.
- Background Codex jobs: check with `node C:/Users/vjump/.claude/plugins/cache/openai-codex/codex/1.0.6/scripts/codex-companion.mjs status`, then `result <task-id>`.
- Local clean-architecture plugin: `C:\Users\vjump\.claude\plugins\cache\anthropic-plugin-directory\clean-architecture\1.0.0-5a68049e6455\CLAUDE.md`. Its per-use-case port rule is intentionally adapted (ADR-0002).
- Commit only when the user asks. Branch off `main` first.

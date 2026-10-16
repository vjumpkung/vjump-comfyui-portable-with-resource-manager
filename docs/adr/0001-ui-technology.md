# ADR-0001: UI technology — Vite + React in the Tauri webview

- Status: Accepted
- Date: 2026-10-15
- Requirements: §2.2, §3.1

## Context
- The brief asks for "all UI as a webview inside a Rust GUI, not Electron".
- ComfyUI's own web UI must be shown inside the app (FR-COMFY-3), so an embedded webview is required whatever toolkit we choose.
- A working React 19 / Tailwind 4 / shadcn frontend already exists in `D:\vjump\vjump-resource-manager-frontend`. It is a single page with a static export and uses no Next server features.

## Options
| | Option | Evidence (2026-10-15) | Assessment |
|---|---|---|---|
| A | Fully native Rust GUI: egui 0.37, iced 0.14, Slint 1.18, gpui 0.2 | Mature toolkits | Contradicts the brief. Still needs a wry child webview for ComfyUI. Full UI rewrite. *Hypothesis, not measured:* mixing a native toolkit with an embedded webview is fragile on Linux/GTK. |
| B | Rust→WASM UI in the webview: Leptos 0.8.22 (0.9 RC), Dioxus 0.7.10 (0.8 alpha) | Both mid major-version transition | One language and shared types. But it can't reuse the existing React components, has fewer ready-made components (xterm and virtual tables need JS interop), and is exposed to upgrade churn. |
| **C** | **Vite 8 + React 19 + TypeScript + Tailwind 4 + shadcn** | Reference frontend is reusable | Reuses presentation components, has a large ecosystem and hot reload. Type safety across the boundary comes from generated bindings (tauri-specta 2 RC). |

## Decision
Option C. The reference frontend's presentation components are reused. Its scattered `axios`/WebSocket calls and global browser events are replaced by feature hooks over one typed IPC gateway.

## Consequences
- There are two languages. Drift is controlled by checked-in generated bindings that CI regenerates and compares (§3.3.5).
- The UI is an outer-layer detail. Domain and application crates have no UI dependency, so a Leptos/Dioxus frontend or a CLI could be added later without changing business rules.
- tauri-specta is a pinned prerelease. If it proves unworkable at M0, fall back to ts-rs 12 for types plus a small, hand-written and tested command wrapper module.
- **Presentation** follows [DESIGN.md](../DESIGN.md) (added 2026-10-15): vjump.dev slate/pastel tokens, shadcn `radix-vega`, lucide-react icons, bundled Inter, Geist Mono and Noto Sans Thai. The reference frontend's `base-vega` components and Remixicon icons are migrated to that style. Only the logic and structure are reused as-is.

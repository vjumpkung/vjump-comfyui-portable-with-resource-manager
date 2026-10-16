# ADR-0002: Clean Architecture + DDD, Rust adaptation

- Status: Accepted (Rust adaptation approved by project owner, 2026-10-15)
- Date: 2026-10-15
- Requirements: §3.3, §6.2, NFR-MAINT

## Context
- The project must scale across several feature areas (environment, ComfyUI runtime, models, custom nodes) and stay easy to maintain.
- The local clean-architecture guidance (plugin `clean-architecture` 1.0.0, `CLAUDE.md`) requires:
  - the inward Dependency Rule;
  - entities / use cases / adapters / frameworks / main layers;
  - screaming architecture;
  - an Input Port interface and an Output Port presenter for every use case.
- Its scaffold uses layer-first folders (`src/entities`, `src/usecases`, …).

## Decision
1. **Layers as crates** with business contexts as modules inside each crate:
   - `domain` ← `application` ← `infrastructure` ← `desktop`, plus `ui/`.
   - Cargo forbids undeclared imports. A CI check on the resolved dependency graph enforces the allowed edges and allow-lists.
   - Contexts get their own crates only when independent evolution or build time justifies it.
2. **Screaming architecture inside each layer.** Modules are named after the contexts: `environment`, `comfy_runtime`, `models::{acquisition, library}`, `custom_nodes`, `settings`, and a `shared` kernel. They are not named after frameworks.
3. **Rust adaptation of ports.** This deviates from the plugin rules:
   - Use cases are concrete structs with `execute(input) -> Result<Output, Error>`.
   - Controllers (Tauri commands) call them directly, and presenters map outputs to serde DTOs.
   - Port traits exist only for dependency inversion (repositories, providers, processes, filesystem, trash, secrets, clock, GPU probe) and for streaming output (`ProgressSink`).
   - They are injected as `Arc<dyn Port + Send + Sync>` using `async-trait`, because native async trait methods are not dyn-compatible.
4. **Serialization stays outward.** No serde or specta derives in `domain` or `application`. DTOs live in `infrastructure` (persistence, providers) and `desktop` (IPC).
5. **Errors per layer.** Each layer has its own `thiserror` enums. Errors are translated at each boundary and sanitized at IPC.
6. **Coordination without an event bus.**
   - One application-level **maintenance gate** serializes environment, node, update and start operations.
   - Required cross-context effects, e.g. download → catalog, are awaited and idempotent.
   - Events and channels are used only for UI notification.
7. **No CQRS or event sourcing.** Query use cases may use read ports. Controllers never touch SQLite.

## Alternatives rejected
- **Strict per-use-case InputPort/OutputPort traits.** About three extra types per use case with no added inversion benefit in Rust, where the crate graph already enforces direction.
- **Crate per context now.** More crates and boilerplate before there is evidence of independent change.
- **Single `core` crate.** The dependency rule could not be enforced mechanically.
- **Tokio broadcast event bus for required effects.** Lagging receivers can drop messages, which would lose catalog registrations.

## Consequences
- Domain and application are testable with no Tauri, network, DB or GPU (NFR-MAINT-3).
- New providers or contexts are additive: a new adapter plus composition-root wiring (NFR-MAINT-2).
- Contributors must follow §3.3 tables. Further deviations require a new ADR (NFR-MAINT-5).

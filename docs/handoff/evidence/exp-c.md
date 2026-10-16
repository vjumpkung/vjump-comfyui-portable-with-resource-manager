# 1. App shell and page registration

There is no URL router: `AppShell` stores the active `NavId` in React state, passes it to `Sidebar`, and `Page` selects the component. The **Custom Nodes** sidebar item is already registered as `id: 'custom-nodes'` with the `Puzzle` icon in `ui/src/app/shell/nav.ts:12-34`. `Sidebar` renders `NAV_ITEMS` and calls `onSelect(item.id)` in `ui/src/app/shell/sidebar.tsx:19-29`.

`ui/src/app/shell/app-shell.tsx:35-67` handles Overview, ComfyUI, Downloads, Models, Settings, and Environment. There is no `custom-nodes` branch yet, so selecting it falls through to the generic “Coming in a later milestone” page. Add the feature import and an `active === 'custom-nodes'` branch there.

Existing feature entry points are small named-export barrels:

- `ui/src/features/library/index.ts:1` exports `LibraryPage`.
- `ui/src/features/downloads/index.ts:1` exports `DownloadsPage`.
- `ui/src/features/environment/index.ts:1` exports `EnvironmentPage`.
- `ui/src/features/comfy-runtime/index.ts:1-3` exports `ComfyPage`, `CloseRequestDialog`, and `RuntimeControls`.

The shell imports feature APIs through those indexes in `ui/src/app/shell/app-shell.tsx:3-8`. The new feature should follow that pattern with `custom-nodes/index.ts`, then be composed in the shell.

Repository guidance: `AGENTS.md:34` specifies TypeScript’s existing style and says generated IPC bindings must not be edited manually; `AGENTS.md:14` keeps Tauri access behind `ui/src/shared/ipc/gateway.ts`. The design doc specifies the sidebar order and Custom Nodes placement at `docs/DESIGN.md:87-93`, with title, body, and metadata styles at `docs/DESIGN.md:72-76`.

# 2. Comparable features: library and downloads

**Files.** Library files under `ui/src/features/library/`: `index.ts`, `library-page.tsx`, `library-page.test.tsx`, `library-store.ts`, `library-store.test.ts`, `use-library.ts`, `use-guarded.tsx`, `model-list.ts`, `model-list.test.ts`, `model-table.tsx`, `details-pane.tsx`, `rename-dialog.tsx`, `move-dialog.tsx`, `delete-dialog.tsx`, and `fixtures.ts`.

Downloads files under `ui/src/features/downloads/`: `index.ts`, `downloads-page.tsx`, `downloads-page.test.tsx`, `downloads-store.ts`, `downloads-store.test.ts`, `use-downloads.ts`, `use-enqueue.tsx`, `queue-table.tsx`, `status-chip.tsx`, `add-download-dialog.tsx`, `choice-dialog.tsx`, `import-pack-dialog.tsx`, `export-dialog.tsx`, `categories.ts`, `category-fields.tsx`, `format.ts`, `format.test.ts`, and `fixtures.ts`.

**Component and state patterns.**

- `LibraryPage` composes header/actions, status alerts and warnings, search/category/root/sort filters, `ModelTable`, `DetailsPane`, and rename/move/delete/replace dialogs (`library-page.tsx:46-73, 144-224, 301-333, 335+`). It accepts `onStopComfy` from the shell (`:53-61`).
- `DownloadsPage` composes its header, connection state, `QueueTable`, add/import/export dialogs, and `useEnqueue` (`downloads-page.tsx:15-27, 28-93`).
- The library and downloads live feeds use external stores with `useSyncExternalStore`, rather than React Query for the primary list (`library/use-library.ts:9-13`, `downloads/use-downloads.ts:5-12`). Their stores call gateway subscription wrappers, which create Tauri `Channel`s. Both release their subscription when the last consumer unmounts; this stops event delivery, not backend work (`library-store.ts:72-99, 120-132`; `downloads-store.ts:73-105, 115-124`).
- Library change notices trigger an authoritative `modelList` refetch; in-flight refetches coalesce to one follow-up (`library-store.ts:47-70`). Downloads apply snapshot/state/progress events and reconnect after a gap (`downloads-store.ts:38-70, 102-105`).
- The partial Custom Nodes hook uses React Query: `NODE_KEY = ['custom-nodes']`, `nodeList` with a five-second refetch interval, and invalidation of `['custom-nodes']` plus `['node-operation']` on events (`custom-nodes/use-nodes.ts:6-17, 24-27`). It subscribes through `gateway.nodeSubscribe`, ignores stale sequence events, retries a failed subscription after two seconds, caps logs at 300 lines, and unsubscribes on cleanup (`:28-53`).

**Virtualization and keyboard behavior.**

- `ModelTable` uses `@tanstack/react-virtual`, virtualizes above `VIRTUALIZE_AFTER = 200`, and keys rows by model identity (`library/model-table.tsx:1, 32-57`; `model-list.ts:4`). It implements an ARIA grid with one row in the tab order. Arrow, Page Up/Down, Home, End, Space, and Delete are handled; it scrolls the virtualizer before restoring focus to a row (`model-table.tsx:59-121, 123-140`).
- `QueueTable` uses a hand-built fixed-height window above 200 rows: 92px rows, 14 visible plus four-row overscan, and spacer rows (`downloads/queue-table.tsx:30-32, 220-256, 273-315`). Its keyboard handling moves focus to adjacent rendered rows on Arrow Up/Down (`:258-271`). This approach does not virtualize distant-row keyboard navigation like `ModelTable` does.

**Dialogs, confirmations, errors, and tests.**

- Library uses dedicated dialogs; downloads uses `ChoiceDialog` for explicit conflict decisions. These use the shared `Dialog`/`Alert`/`Button` components.
- Errors are a mix of inline state and Sonner toasts: library scan failure uses `toast.error` and scan warnings/recovery are inline (`library-page.tsx:102-108, 191-223`); downloads actions use React Query `useMutation` with `toast.error(errorMessage(error))`, while per-job failures are also displayed inline (`queue-table.tsx:40-45, 129-138`). The app mounts `<Toaster />` in `ui/src/app/App.tsx:114-124`; `ErrorNotice` is in `ui/src/shared/ui/feedback.tsx:7-20`.
- Page tests are colocated as `*.test.tsx` and use Vitest plus Testing Library. They partially mock the gateway with `vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({ ...(await importOriginal()), gateway: { ...vi.fn() } }))`, capture subscription callbacks in an `emit` variable, and mount pages under a fresh `QueryClientProvider` (`library-page.test.tsx:1-43, 45-88`; `downloads-page.test.tsx:1-31, 33-69`). Store tests inject a small mock API into `createLibraryStore`/`createDownloadsStore` and invoke captured event handlers directly (`library-store.test.ts:9-27`; `downloads-store.test.ts:8-22`).

# 3. Available shared UI components

The `.tsx` files in `ui/src/shared/ui/` are: `alert.tsx`, `badge.tsx`, `button.tsx`, `card.tsx`, `checkbox.tsx`, `dialog.tsx`, `empty-state.tsx`, `feedback.tsx`, `field.tsx`, `input.tsx`, `label.tsx`, `log-view.tsx`, `progress.tsx`, `radio-group.tsx`, `separator.tsx`, `skeleton.tsx`, `sonner.tsx`, `status-badge.tsx`, `table.tsx`, `toggle-group.tsx`, `toggle.tsx`, and `tooltip.tsx`. There is also `log-view.test.tsx`. Evidence: `rg --files ui/src/shared/ui`.

# 4. Gateway and generated bindings

**Gateway wrappers currently present** in `ui/src/shared/ipc/gateway.ts:215-244`:

- `nodeRegistrySearch(input: RegistrySearchInput)` → `Promise<RegistryPageDto>`
- `nodeList()` / `nodeRescan()` → `Promise<NodeListDto>`
- `nodePrepareRegistry(input: PrepareRegistryInput)`, `nodePrepareGit(input: PrepareGitInput)`, `nodePrepareUpdate(input: PrepareUpdateInput)`, `nodePrepareRetryDeps(input: PrepareRetryInput)` → `Promise<PlanReviewDto>`
- `nodeCommit(input: CommitInput)` → `Promise<NodeOutcomeDto>`
- `nodeSetEnabled(input: SetEnabledInput)` → `Promise<NodeOutcomeDto>`
- `nodeUninstall(nodeId: string)` → `Promise<NodeOutcomeDto>`; wraps the ID as `{ nodeId }` for bindings.
- `nodeUpdateAll(items: CommitInput[])` → `Promise<BatchOutcomeDto>`; wraps as `{ items }`.
- `nodeCancel(key: string)` → `Promise<null>`
- `nodeOperationStatus(operationId: string)` → `Promise<OperationDto>`
- `nodeSubscribe(onEvent: (event: NodeEventDto) => void)` → `Promise<number>`
- `nodeUnsubscribe(subscriptionId: number)` → `Promise<null>`

`nodeSubscribe` creates the `Channel<NodeEventDto>` behind the gateway (`gateway.ts:237-244`). Gateway errors become `IpcClientError(code, message)`; arbitrary transport exceptions become the generic `transport` error “Could not reach the application backend.” (`gateway.ts:59-89`).

**Bindings command functions** are at `ui/src/shared/ipc/bindings.ts:92-123` and match the node command set above. `nodeOpenRepository` is **absent** from `bindings.ts` and `gateway.ts`. The Rust command is registered in `crates/desktop/src/ipc/bindings.rs:71` and implemented as `node_open_repository` at `crates/desktop/src/ipc/node_commands.rs:102`. The handoff confirms the gateway wrapper is missing and `bindings.ts` predates that command (`HANDOFF.md:86, 90-96`).

The node command DTO definitions in `bindings.ts` are under 200 lines total; copied here verbatim. Supporting types used by the node DTOs are included too.

```ts
export type BatchItemDto = {
	planId: string,
	outcome: NodeOutcomeDto | null,
	error: IpcError | null,
};

export type BatchOutcomeDto = {
	items: BatchItemDto[],
	/**  The batch stopped because the environment now needs repair. */
	stoppedForRepair: boolean,
};

/**  The user's decision for one reviewed plan. `install.py` defaults to unchecked in the UI. */
export type CommitInput = {
	planId: string,
	binding: string,
	executeNodeCode: boolean,
	runInstallPy: boolean,
	acknowledgeWarnings: boolean,
	rememberSource: boolean,
	rememberInstallPy: boolean,
	confirmedChanges: PinChangeDto[],
};

export type DependencyStateDto = "not_run" | "ok" | "failed";

export type InstalledNodeDto = {
	id: string,
	name: string,
	enabled: boolean,
	singleFile: boolean,
	source: NodeSourceDto,
	origin: OriginDto,
	dependencies: DependencyStateDto,
	/**  The provenance on disk disagrees with the stored record. */
	conflict: boolean,
	/**  Local changes of a Git node; unknown when Git could not be asked. */
	dirty: boolean | null,
};

/**  Sanitized error crossing the IPC boundary: no paths, secrets or raw backend text. */
export type IpcError = {
	code: string,
	message: string,
};

export type NodeEventDto = { kind: "snapshot"; epoch: string; sequence: number | null } | { kind: "changed"; epoch: string; sequence: number | null } | { kind: "log"; epoch: string; sequence: number | null; operationId: string; line: string } | { kind: "gap"; epoch: string; sequence: number | null };

export type NodeIdInput = {
	nodeId: string,
};

export type NodeListDto = {
	nodes: InstalledNodeDto[],
	/**  Operations that are not completed or failed; a start is blocked while one is unresolved. */
	unresolved: OperationDto[],
	/**  The selected environment needs repair; most node operations are limited until then. */
	repairRequired: boolean,
};

export type NodeOutcomeDto = {
	operationId: string,
	node: InstalledNodeDto | null,
	/**  A retained backup could not be moved to the Recycle Bin yet. */
	cleanupPending: boolean,
};

export type NodeSourceDto = { kind: "registry"; id: string; publisher: string | null; version: string; repository: string | null } | { kind: "git"; repository: string; commit: string; branch: string | null } | { kind: "manual" };

export type OperationDto = {
	id: string,
	kind: OperationKindDto,
	/**  One of the journal phases, for example `publish_intent` or `completed`. */
	phase: string,
	nodeName: string,
	failure: string | null,
	logTail: string[],
};

export type OperationKindDto = "install" | "update" | "retry_dependencies" | "enable" | "disable" | "uninstall";

export type OriginDto = "app" | "observed";

export type PinChangeDto = {
	name: string,
	from: string | null,
	to: string | null,
};

export type PlanReviewDto = {
	id: string,
	binding: string,
	kind: OperationKindDto,
	name: string,
	source: NodeSourceDto,
	/**  The source before an update. */
	priorSource: NodeSourceDto | null,
	requirements: string[],
	artifactDigest: string | null,
	treeDigest: string,
	hookDigest: string | null,
	protectedChanges: PinChangeDto[],
	warnings: string[],
	trustedSource: boolean,
	trustedInstallPy: boolean,
	hasRequirements: boolean,
	hasInstallPy: boolean,
};

export type PrepareGitInput = {
	requestId: string,
	repository: string,
	/**  Branch, tag or commit; absent means the default branch. */
	reference: string | null,
};

export type PrepareRegistryInput = {
	/**  Client-chosen key that `node_cancel` accepts while the preparation runs. */
	requestId: string,
	id: string,
	/**  Absent means the latest release, resolved once and shown in the review. */
	version: string | null,
};

export type PrepareRetryInput = {
	requestId: string,
	nodeId: string,
};

export type PrepareUpdateInput = {
	requestId: string,
	nodeId: string,
	/**  Required for detached or upstream-less Git nodes; a tag, branch or commit. */
	reference: string | null,
};

export type RegistryNodeDto = {
	id: string,
	name: string,
	publisher: string | null,
	description: string | null,
	downloads: number | null,
	/**  GitHub stars as reported by the registry; unknown stays null. */
	githubStars: number | null,
	latestVersion: string | null,
	repository: string | null,
	os: SupportDto,
	accelerator: SupportDto,
	comfyui: SupportDto,
	flagged: boolean,
	deprecated: boolean,
	warnings: string[],
};

export type RegistryPageDto = {
	nodes: RegistryNodeDto[],
	page: number,
	limit: number,
	total: number | null,
	totalPages: number,
	fetchedAt: string,
	/**  Served from the cache because the registry could not be reached. */
	stale: boolean,
};

export type RegistrySearchInput = {
	search: string,
	page: number,
	limit: number,
	/**  Bypass the cache lifetime for this query. */
	refresh: boolean,
};

export type SetEnabledInput = {
	nodeId: string,
	enabled: boolean,
};

export type SupportDto = "compatible" | "incompatible" | "unknown";

export type UpdateAllInput = {
	items: CommitInput[],
};
```

Source ranges: `bindings.ts:135-145, 190-199, 257, 423-441, 535-578, 613-617, 655-700, 761-797, 911-915, 1002, 1013-1015`. Note that `NodeEventDto.sequence` is nullable in these bindings; the Rust DTO declares `f64` and currently sends numeric values (`node_dto.rs:407-424`, `node_commands.rs:357-379`).

# 5. Backend prepare, consent, operations, and events

**Prepare → review → commit.** The prepare commands accept registry, Git, update, or dependency-retry requests and return `PlanReviewDto` (`crates/desktop/src/ipc/node_commands.rs:158-215`). Registry version may be omitted to resolve latest once; Git reference may be omitted for its default branch; Git update reference is required for detached/upstream-less nodes (bindings `:675-700`). Review includes source and prior source, requirements, artifact/tree/hook digests, protected package changes, warnings, trust indicators, and `hasRequirements`/`hasInstallPy` (`crates/desktop/src/ipc/node_dto.rs:299-317`).

Commit must echo **`planId` and `binding`** from the review. It does **not** take digest fields: the backend compares the current tree, requirements, and hook digests to the stored review and rejects a stale plan (`workflow.rs:440-445`). `confirmedChanges` must equal the plan’s protected changes; the backend sorts and compares the lists (`workflow.rs:419-425`).

Consent details:

- `executeNodeCode` must be `true` for every commit; warnings require `acknowledgeWarnings: true`. `runInstallPy` is valid only when the plan has the hook. `rememberInstallPy` requires both `rememberSource` and `runInstallPy` (`domain/custom_nodes/consent.rs:14-21`).
- The IPC adapter sets `allow_source_builds: false`; the frontend cannot authorize source builds, and the backend rejects that flag if true (`node_commands.rs:40-61`; `workflow.rs:416-418`).
- Remembered trust is keyed to a normalized Git repository, or to a Registry source with ID, publisher, and repository (`domain/custom_nodes/installed_node.rs:111-127`). On commit it stores `allow_install_py` from `rememberInstallPy` (`workflow.rs:523-532`). A later review reports `trustedSource` and `trustedInstallPy` (`workflow.rs:217-225, 251-255`), but those values do not replace the required per-plan `executeNodeCode` and `runInstallPy` consent.
- The hook runs only when `runInstallPy` is true; failure leaves the node dependency state failed and the environment needing repair (`workflow.rs:480-505, 709-724`). Review warns that the venv is not a sandbox (`workflow.rs:227-229`).

`node_update_all` takes 1–128 reviewed `CommitInput`s, runs them sequentially under one maintenance lease, and returns per-plan outcome/error entries plus `stoppedForRepair` (`node_commands.rs:262-300`). `nodeCancel(key)` cancels a preparation/commit by key or discards a reviewed plan; cancellation after dependency mutation can leave the environment requiring repair (`node_commands.rs:308-324`; bindings `:112-116`). Cleanup/unmount should unsubscribe from events, not call cancel.

**List and operation state.** `NodeListDto` carries installed nodes, unresolved operations, and `repairRequired` (`node_dto.rs:181-188`; TypeScript `bindings.ts:541-547`). `nodeOperationStatus(operationId)` returns one operation with its phase, failure, and current log tail (`node_commands.rs:329-338`; `node_dto.rs:157-178`). `nodeRescan` finishes interrupted operations and rereads folders; it also runs after each verified ComfyUI stop (`node_commands.rs:146-155`).

**Events.** A subscription starts with a snapshot marker and current sequence, then sends ordered `changed` or `log` events. A slow subscriber receives `gap`; the DTO instructs the UI to refetch the node list and operation state (`node_commands.rs:341-384`; `node_dto.rs:407-424`). The event epoch identifies the stream instance; sequence increases per event. The snapshot contains no list payload. Unsubscribing aborts the event task only (`node_commands.rs:398-413`).

# 6. Partial Custom Nodes files

There are only three files in `ui/src/features/custom-nodes/`; no page, `index.ts`, or colocated tests are present (`rg --files ui/src/features/custom-nodes`). The handoff identifies those missing page/index/shell route and lists the intended page scope at `HANDOFF.md:104`.

- `model.ts` exports `sourceLabel`, `Consent`, `EMPTY_CONSENT`, `canCommit`, and `decision` (`:3-42`). It builds a commit with the review ID/binding and the pin-change list.
- `use-nodes.ts` exports `NODE_KEY` and `useNodes` (`:6-56`). It polls the list, subscribes to events, invalidates list/operation queries, retries subscription, and keeps a bounded log buffer.
- `review-dialog.tsx` exports `ReviewDialog`; `PlanConsent` is internal (`:12-59, 61-90`). It displays source, hashes, requirements, warnings, protected changes, and execution/script/trust controls.

**Concrete consent mismatches to fix before relying on the dialog:**

1. `canCommit` allows `trustedSource` to bypass the execute checkbox, but `decision` sends `executeNodeCode: consent.execute` (`model.ts:25-40`). The dialog hides that checkbox for trusted sources (`review-dialog.tsx:49-50`), while backend consent always requires `executeNodeCode: true` (`consent.rs:15-20`). That path can enable Apply and then be rejected.
2. `rememberInstallPy` is derived from `consent.rememberHook` and `consent.remember`, but unchecking Remember source does not clear the hidden `rememberHook` state (`review-dialog.tsx:54-56, 67-80`; `model.ts:31-40`). It can submit `rememberInstallPy: true` with `rememberSource: false`, which backend consent rejects.
3. The dialog state is keyed by plan ID, not binding (`review-dialog.tsx:67-80`). Its child key includes the binding, but the parent consent map is retained by plan ID; explicitly reset consent when the reviewed plan/binding changes.
4. The trust checkbox is offered when a non-manual source has a repository (`review-dialog.tsx:55`), but Registry trust also needs a publisher; otherwise the backend has no trust namespace to save (`installed_node.rs:112-127`).
5. The dialog says `hasRequirements` means “Includes root requirements.txt” (`review-dialog.tsx:34`), but backend sets it for either root requirements or registry dependency requirements (`workflow.rs:230-255`).

# 7. Environment Repair facade and boundaries

`EnvironmentPage` receives `onSetup: (mode: SetupModeDto) => void` and invokes `onSetup('repair')` for Repair (`ui/src/features/environment/environment-page.tsx:72-80, 129-141`). `AppShell` passes that callback to the page (`ui/src/app/shell/app-shell.tsx:25-29, 54-57`); `Workspace` in `ui/src/app/App.tsx:90-97` turns it into wizard state. For Custom Nodes, the shell can pass a small `onRepair` callback that calls the existing `onSetup('repair')`; the node feature need not import Environment internals.

ESLint defines `app`, `feature`, `shared-ipc`, and `shared` elements and disallows feature-to-feature dependencies (`ui/eslint.config.js:37-44, 70-84`). It also blocks direct imports of generated `bindings.ts` and `@tauri-apps/*` from app/features/shared (`:91-101`). Use the gateway for node and Comfy IPC.

# 8. ComfyUI stop, restart, and status

The gateway exposes `comfyStatus`, `comfyStart`, `comfyStop`, `comfyRestart`, `comfySubscribe`, and `comfyUnsubscribe` (`ui/src/shared/ipc/gateway.ts:133-148`). `comfy-runtime/use-runtime.ts` owns the feature-local live status hook and mutation helpers; restart/stop settle by refreshing runtime state and invalidating relevant queries (`:15-40`). `ComfyPage` uses those helpers and has Restart and Stop controls (`comfy-page.tsx:120-140, 220-243`). The feature index exports `ComfyPage` and `RuntimeControls`, but does not export `useRuntime` (`comfy-runtime/index.ts:1-3`).

The established cross-feature bridge is at the app-shell level: `app-shell.tsx:20-23` calls `gateway.comfyStop()` and passes it to `LibraryPage` as `onStopComfy` (`:46`). For Custom Nodes, the shell can similarly inject Stop and Restart callbacks. The current Comfy status store is feature-local; Custom Nodes should not import `useRuntime` from the Comfy feature. Status can be obtained through the gateway or passed through app-shell composition.

## Open uncertainties

- The generated `bindings.ts` predates `node_open_repository`; the handoff says to regenerate it after finishing the command integration (`HANDOFF.md:90-100`). Until then, a repository-open UI action has no generated binding or gateway wrapper.
- The handoff marks M5 backend findings unresolved (`HANDOFF.md:3, 106-112`), including repair restoring node dependencies. Repair flow behavior should be checked against those findings when implementing recovery UI.
- No Custom Nodes page tests or render/type checks exist; the handoff says the review dialog was not typechecked or render-checked (`HANDOFF.md:89`). This was a read-only inspection; I did not run Cargo or pnpm.
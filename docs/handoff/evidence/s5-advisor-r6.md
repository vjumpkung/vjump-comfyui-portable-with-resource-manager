**NO-GO on R6 in the current implementation.** The blocker is the **pre-consent Windows preparation path**. DLL loading after execution consent can remain a documented residual under the existing “third-party code; venv is not a sandbox” boundary.

This classification is **AI-decidable against the approved requirements**. Waiving the pre-consent exposure, or redefining preparation as permission to execute node code, is **owner-only**: it changes [NFR-SEC-3](/D:/vjump/vjump-comfyui-portable-with-resource-manager/docs/REQUIREMENTS.md:473).

**R6-1: Evidence and release reasoning**

The consent boundary comes after preparation:

- Preparation calls dependency preview at [workflow.rs:301](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:301), then constructs the review at [workflow.rs:343](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:343).
- Explicit execution consent is checked during commit at [workflow.rs:549](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:549).
- The UI likewise waits for preparation before showing the review: [use-node-actions.ts:147](/D:/vjump/vjump-comfyui-portable-with-resource-manager/ui/src/features/custom-nodes/use-node-actions.ts:147).

| Process/site | Working directory and timing | Disposition |
|---|---|---|
| **uv dependency preview** | `preview_install` applies `with_guarded` at [packages.rs:414](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:414), which sets node cwd at [packages.rs:91](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:91). Called before review/consent through [workflow.rs:451](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:451). | **Confirmed pre-consent exposure.** This runs `uv pip install --dry-run`, rather than `pip compile`; see [packages.rs:46](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:46). |
| **uv actual install** | Uses the same guarded cwd through [packages.rs:375](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:375). Node installation occurs at [workflow.rs:850](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:850), after commit consent. | Acceptable post-consent residual. |
| **Python BOOT / `install.py`** | Explicit node cwd at [hook.rs:69](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/hook.rs:69). Called only when `run_install_py` is selected at [workflow.rs:857](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/application/src/custom_nodes/workflow.rs:857). | Acceptable post-consent residual. BOOT’s integrity check does not establish a native DLL-loading boundary. |
| **Git node operations** | `GitNodes::spec` supplies **`-C <node>`**, not `CommandSpec.cwd`: [git.rs:227](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:227). Nevertheless, Git changes cwd internally. This includes config preflight [git.rs:302](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:302), staged checkout [git.rs:531](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:531), status [git.rs:642](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:642), and update fetch [git.rs:781](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/git.rs:781). Staging and subsequent inspection occur during preparation: [files.rs:465](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:465), [files.rs:503](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/files.rs:503). | **Pre-consent coverage must be resolved**, including Git helpers. A safe initial cwd alone does not cover later internal changes. |
| **Other Python checkout cwd sites** | Runtime [process.rs:407](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/process.rs:407); CLI preflight [process.rs:795](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/process.rs:795); capability probe [process.rs:818](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/comfy_runtime/process.rs:818); final verification [verify.rs:206](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/verify.rs:206), applied at [verify.rs:128](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/verify.rs:128). | These use the ComfyUI checkout, rather than a newly staged node directory. They belong to the installed-code execution boundary. |

The shared runner applies `CommandSpec.cwd` directly at [process.rs:480](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process.rs:480). Commands without one inherit the manager’s cwd; [layout.rs:235](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/layout.rs:235) supplies no safe default. HF explicitly uses the installation root at [hf_engine.rs:87](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/models/acquisition/hf_engine.rs:87).

**The attacker need not already control the user’s account.** A malicious node publisher can include a DLL in the repository/archive subsequently staged by the application. Choosing that source for review does not grant the later execution consent. Filesystem containment and hashes preserve identity; they do not make its contents trusted.

Exploitation still requires a tool or dependency to request a matching DLL through a search that reaches cwd. SafeDllSearchMode places cwd after system directories and before PATH; it does not remove it. This review **does not establish an exploitable DLL name in pinned uv or MinGit**. That limits the claim to an unclosed execution boundary, rather than demonstrated exploitation of those binaries. [Microsoft DLL search documentation](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order).

The current test does **not** prove inherited child protection:

- It plants an **invalid** DLL and launches the helper in that directory: [tests.rs:674](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/tests.rs:674).
- Before initialization, the helper accepts **either 193 or 126**: [tests.rs:712](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/tests.rs:712).
- It then initializes policy **inside the helper**, and requires 126: [tests.rs:716](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/tests.rs:716).

Thus the reported 193 means the invalid cwd file was selected for an attempted load—not that DLL code executed. The committed assertion primarily proves process-local protection. It also does not test startup import resolution.

**R6-2: Corrected implementation plan**

The release invariant should be:

> Before exact-source execution consent, node-controlled directories must not become DLL search locations for tools or their descendants.

The smallest **confirmed exposure** to fix is uv preview. A bounded follow-up should:

1. **Build Windows previews from a trusted cwd.** Change `UvPackages::preview_install` and its preparation helpers in [packages.rs](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/environment/packages.rs:405). Preserve guarded root bytes, retention, wheel-only resolution and fail-closed preflight.
2. **Preserve relative requirements explicitly.** Normalize the already-supported requirement syntax into retained preview inputs: nested `-r`/`-c` references and local wheel paths need explicit absolute locations. Changing cwd while retaining the existing stdin unchanged is insufficient. The compatibility fixture expressly depends on these paths: [tests.rs:5642](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tests.rs:5642).
3. **Close Git’s pre-consent coverage separately.** Review `GitNodes::{spec,run,clone_checkout,inspect,freeze_update}` and scan callers in `custom_nodes/files.rs`. Require evidence covering the pinned MinGit executable and helpers after `-C`. If that protection cannot be established, these operations need an earlier exact-source execution-consent gate or a redesigned preparation path. Replacing `-C` with explicit repository arguments alone is not sufficient evidence.
4. **Correct misleading policy comments and tests.** Update [windows.rs:27](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/shared/process/windows.rs:27), approved-plan DLL claims, and residual row 6. Separate the process-local initializer test from any child-protection test.

Required verification:

- An uninitialized helper and grandchild must satisfy the claimed protection **without calling the initializer themselves**.
- Windows preview uses trusted cwd and preserves nested includes, constraints and local wheels with **pinned uv 0.13.1**.
- Keep `node_preview_never_executes_editable_build_backend`.
- Cover Git preparation, inspection and helpers; include newly staged content and manually discovered nodes.
- Preserve existing cleanup/retention and post-consent hook behavior.

I would **not certify this as a safe one-line change now**. The approved plan’s compatibility fixture currently asserts node cwd at [tests.rs:5681](/D:/vjump/vjump-comfyui-portable-with-resource-manager/crates/infrastructure/src/custom_nodes/tests.rs:5681). Closure requires implementation plus native verification.

`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY` is not itself a cwd-exclusion mechanism. For example, `PreferSystem32Images` changes ordering, while remote/low-label restrictions do not reject ordinary local node directories. A launcher that only initializes itself likewise needs proof of protection in the actual tools. [Microsoft image-load policy](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_image_load_policy).

**R6-3: Exact documentation wording**

For `docs/RELEASE_ACCEPTANCE.md` **now**—the file currently does not exist:

> **R6 — BLOCKED, Windows; NFR-SEC-3.** Guarded dependency preview runs uv with node-controlled cwd before execution consent. Git preparation also changes cwd internally through `-C`. The process-local DLL initializer does not establish persistent runtime protection in tool children. The helper test accepts cwd selection before initializing itself. No pinned-tool exploit is claimed. Release requires closure of the pre-consent boundary and recorded native evidence, or an explicit owner-approved exception to NFR-SEC-3.

After that boundary is closed, the following **post-consent residual** can be accepted.

README Security notes:

> Custom nodes and their dependencies run with your user account’s permissions; a Python virtual environment is not a sandbox. On Windows, tools running in an approved node directory may load native DLLs supplied by that node. Approve execution only for sources you trust.

DOCUMENTATION §27:

> **Windows native-library boundary.** The manager’s DLL search initialization is process-local and does not establish the same runtime policy in uv, Git, Python or their descendants. Pre-consent preparation must keep node-controlled directories out of tool DLL searches or refuse the operation. After exact-source execution consent, node requirements and explicitly approved `install.py` scripts may run with node-directory working semantics, including native-library loading. This is third-party execution with the user’s permissions.

Release ledger after verified closure:

> **R6 — ACCEPTED POST-CONSENT RESIDUAL.** No inherited DLL-policy guarantee is claimed. Pre-consent exposure was closed by [implementation reference], verified by [native evidence reference]. DLL loading during explicitly approved third-party execution remains within the documented non-sandbox boundary.

**No indispensable user questions for the recommended disposition.** Accepting the current pre-consent gap would require an explicit owner waiver; a documentation warning alone would not satisfy the existing requirement.

Read-only review completed. No files changed, Git writes, builds, or tests were run.
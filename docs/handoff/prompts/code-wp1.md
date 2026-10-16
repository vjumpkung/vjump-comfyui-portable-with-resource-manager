ROLE: coder (write-capable). Mechanically implement the approved plan below. Do not redesign or expand scope. If underspecified or conflicting with the code, stop and report the gap.

WORKING COPY: C:\Users\vjump\Documents\vjump-comfyui-portable-with-resource-manager (main repo). Another coder concurrently edits files under ui/ only: do not touch ui/. Do not commit.

MANDATORY: read C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\coder-env.md. In Git Bash `source /c/Users/vjump/AppData/Local/Temp/claude/C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager/ce429ed4-f6f3-49eb-9e87-13e9ba4d84af/scratchpad/gnu-env.sh` before every cargo command.

GOAL: M5 backend finding 7 — operation-time filesystem containment for custom-node mutations (NFR-SEC-5 incl. check/use races).

APPROVED SPEC (authoritative): C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\adv-m5-backend.out.md section "WP1 — Implement operation-time filesystem containment" (PinnedDirectory::open_entry/remove_owned_tree, directory-capable shared rename_no_replace, pinned recycle_entry with no permanent-delete fallback and Linux directory-relative trash placement or safe failure retaining the backup, new custom_nodes/paths.rs NodePaths with retained pins, route ALL node mutations through protected parents, new Recycler type, remove node use of path-based tree::rename_no_replace, error mapping, and the four named tests). Evidence: C:\Users\vjump\AppData\Local\Temp\claude\C--Users-vjump-Documents-vjump-comfyui-portable-with-resource-manager\ce429ed4-f6f3-49eb-9e87-13e9ba4d84af\scratchpad\exp-b.out.md "Finding 7" and "Reusable helpers". Windows-only and Unix-only tests must be cfg-gated so each platform runs its own; on this Windows machine use directory junctions (creatable without admin, e.g. via std::process::Command "cmd /C mklink /J") for substitution tests where symlinks need privileges, or skip with a clear #[cfg]/runtime guard when creation is not permitted — report which ran.
Keep the model-library behaviour unchanged (its tests must still pass). Update the NodeFiles port doc in application custom_nodes/ports.rs to state exactly what is guaranteed.

SUCCESS CRITERIA (GNU env): cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test -p infrastructure --locked; cargo test -p application custom_nodes --locked; cargo test -p desktop --test acl --locked; node scripts/check-arch-deps.mjs.

OUTPUT: per-file summary (file:line), new/changed public signatures, tests added (names) and which ran on Windows, exact results.

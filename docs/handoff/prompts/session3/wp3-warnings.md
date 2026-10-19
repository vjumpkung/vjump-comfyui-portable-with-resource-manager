ROLE: coder (write-capable). Small follow-up fix to M5 WP3 in the main working copy. Do not redesign or expand scope.

ADVISOR DECISION (authoritative, accepted by main): registry review warnings for an exact release =
- OS, accelerator and ComfyUI compatibility evaluated EXCLUSIVELY from the selected exact release's metadata (the `/nodes/{id}/install?version=` response). Missing/unparseable dimensions stay `Unknown` (they produce the existing "... compatibility is unknown" warnings). Never borrow compatibility from the node's `latest_version` or node-level fields.
- flagged/deprecated = the exact release's flags OR the TOP-LEVEL node's own `status`/`deprecated` from the `/nodes/{id}` detail. Flags that appear only on an unrelated `latest_version` object must NOT propagate to another version's review.
- Call `Compatibility::warnings()` once on that combined value (fixed dimension-first order, flags after, naturally duplicate-free). Registry detail errors keep propagating (registry.node(id).await?), acknowledgement behaviour unchanged.

CURRENT CODE: crates/infrastructure/src/custom_nodes/files.rs `review_warnings(release, node)` (~line 145, called ~line 408 with `node.evaluated`) concatenates node and release warnings; crates/infrastructure/src/custom_nodes/registry.rs `parse_node` (~line 296) folds latest-version metadata and node-level flags into `RegistryNode.evaluated`, so provenance is lost. Keep `parse_node`'s browse-list behaviour (node-level flags shown on the listing) unchanged; preserve the top-level node flags separately for review with the smallest change that keeps layering rules (no serde in application; if `RegistryNode` is an application type, prefer the minimal additive field or read the top-level flags inside infrastructure where the detail is fetched; report the choice). If a desktop DTO changes, regenerate bindings via the export test.

TESTS (crates/infrastructure/src/custom_nodes/tests.rs `registry_node_and_release_warnings_are_bound_to_review`, ~line 807). The fixture injects host Windows/CUDA/ComfyUI 0.3.50 on every runner; base assertions on that injected host, never on cfg!(windows).
1. Make the conflict portable: latest_version `supported_os: ["macOS"]`, exact release `supported_os: ["OS Independent"]` (see crates/domain/src/custom_nodes/compatibility.rs OS classifier).
2. Expected warnings exactly: ["accelerator is incompatible", "ComfyUI is incompatible", "Registry status is flagged or unavailable", "This node is deprecated"]; change any frozen-evidence length 5 -> 4; keep the repeated-preparation equality.
3. Replace release-only substring assertions with equality to the same vector; a metadata-free active node detail adds no unknown warnings.
4. Add cases: node-only flags survive when exact-release flags are clear; exact ["macOS"] with latest ["OS Independent"] keeps "OS is incompatible" first; exact compatibility entirely missing with latest fully compatible gives exactly the three "... compatibility is unknown" warnings; flags present only on the latest release do not propagate.
Keep `incompatible_registry_metadata_becomes_review_warnings` and the other existing registry tests passing without weakening them.

SUCCESS CRITERIA (report each):
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -j 4 -- -D warnings
- cargo test -p infrastructure --locked -j 4 custom_nodes ; cargo test --workspace --locked -j 4 --no-fail-fast
- node scripts/check-arch-deps.mjs

OUTPUT: file:line summary, chosen shape for the node-wide flags, tests changed/added, exact results.

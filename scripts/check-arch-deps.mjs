import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const allowedDependencies = new Map([
  ['domain', new Set(['thiserror'])],
  ['application', new Set(['domain', 'thiserror', 'async-trait', 'futures-core'])],
]);

const allowedWorkspaceEdges = new Map([
  ['domain', new Set()],
  ['application', new Set(['domain'])],
  ['infrastructure', new Set(['application', 'domain'])],
  ['desktop', new Set(['application', 'infrastructure', 'domain'])],
]);

export function check(metadata) {
  const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
  const workspace = new Set(metadata.workspace_members);
  const workspaceNames = new Set(metadata.workspace_members.map((id) => packages.get(id)?.name));
  const nodes = new Map((metadata.resolve?.nodes ?? []).map((node) => [node.id, node]));
  const violations = new Set();

  for (const id of workspace) {
    const pkg = packages.get(id);
    if (!pkg) {
      violations.add(`Missing workspace package identity: ${id}`);
      continue;
    }

    function inspect(name, isWorkspaceDependency) {
      const allowList = allowedDependencies.get(pkg.name);
      if (allowList && !allowList.has(name)) {
        violations.add(`${pkg.name}: forbidden direct dependency "${name}"`);
      }
      if (pkg.name === 'infrastructure' && (name === 'desktop' || name.startsWith('tauri'))) {
        violations.add(`infrastructure: forbidden direct dependency "${name}"`);
      }
      if (isWorkspaceDependency && !allowedWorkspaceEdges.get(pkg.name)?.has(name)) {
        violations.add(`${pkg.name}: forbidden workspace edge to "${name}"`);
      }
    }

    // Resolved package identity catches renamed dependencies, across every kind/target.
    for (const dependency of nodes.get(id)?.deps ?? []) {
      const target = packages.get(dependency.pkg);
      if (!target) {
        violations.add(`${pkg.name}: unresolved dependency identity "${dependency.pkg}"`);
        continue;
      }
      inspect(target.name, workspace.has(target.id));
    }

    // Declarations also catch optional or target dependencies absent from the graph.
    // Cargo's `name` is the original package name; `rename` is only the local alias.
    for (const dependency of pkg.dependencies) {
      inspect(dependency.name, workspaceNames.has(dependency.name));
    }
  }
  return [...violations];
}

function main() {
  const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--locked', '--all-features'], {
    cwd: fileURLToPath(new URL('../', import.meta.url)),
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    console.error('Architecture check: cargo metadata failed.');
    if (result.stderr) process.stderr.write(result.stderr);
    if (result.error) console.error(result.error.message);
    return 1;
  }
  let metadata;
  try {
    metadata = JSON.parse(result.stdout);
    if (!metadata.resolve) throw new Error('resolved dependency graph is missing');
  } catch (error) {
    console.error(`Architecture check: invalid cargo metadata (${error.message}).`);
    return 1;
  }
  const violations = check(metadata);
  for (const violation of violations) console.error(violation);
  if (violations.length) return 1;
  console.log('Architecture dependencies: OK');
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}

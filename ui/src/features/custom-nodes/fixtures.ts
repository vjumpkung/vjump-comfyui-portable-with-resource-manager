import type {
  InstalledNodeDto,
  NodeGitStatusDto,
  NodeListDto,
  NodeOutcomeDto,
  OperationDto,
  PlanReviewDto,
  RegistryNodeDto,
  RegistryPageDto,
} from '@/shared/ipc/gateway'

export function node(overrides: Partial<InstalledNodeDto> = {}): InstalledNodeDto {
  return {
    id: 'node-1',
    name: 'ComfyUI-Example',
    enabled: true,
    singleFile: false,
    source: {
      kind: 'git',
      repository: 'https://github.com/example/ComfyUI-Example',
      commit: 'a'.repeat(40),
      branch: 'main',
    },
    origin: 'app',
    dependencies: 'ok',
    conflict: false,
    dirty: false,
    ...overrides,
  }
}

export function registryInstalled(
  overrides: Partial<InstalledNodeDto> = {},
): InstalledNodeDto {
  return node({
    source: {
      kind: 'registry',
      id: 'example-pack',
      publisher: 'example',
      version: '1.2.3',
      repository: 'https://github.com/example/example-pack',
    },
    dirty: null,
    ...overrides,
  })
}

export function manyNodes(count: number): InstalledNodeDto[] {
  return Array.from({ length: count }, (_, index) =>
    registryInstalled({
      id: `node-${String(index).padStart(4, '0')}`,
      name: `Pack ${String(index).padStart(4, '0')}`,
    }),
  )
}

export function plan(overrides: Partial<PlanReviewDto> = {}): PlanReviewDto {
  return {
    id: 'plan-1',
    binding: 'binding-1',
    kind: 'install',
    name: 'example-pack',
    source: {
      kind: 'registry',
      id: 'example-pack',
      publisher: 'example',
      version: '1.2.3',
      repository: 'https://github.com/example/example-pack',
    },
    priorSource: null,
    requirements: ['numpy>=1.26'],
    artifactDigest: 'b'.repeat(64),
    treeDigest: `git:${'c'.repeat(40)}`,
    hookDigest: null,
    protectedChanges: [],
    warnings: [],
    trustedSource: false,
    trustedInstallPy: false,
    hasRequirements: true,
    hasInstallPy: false,
    ...overrides,
  }
}

export function operation(overrides: Partial<OperationDto> = {}): OperationDto {
  return {
    id: 'op-1',
    nodeId: 'node-1',
    blocksStart: false,
    kind: 'install',
    phase: 'completed',
    nodeName: 'ComfyUI-Example',
    failure: null,
    logTail: [],
    ...overrides,
  }
}

export function nodeList(overrides: Partial<NodeListDto> = {}): NodeListDto {
  return {
    nodes: [node()],
    unresolved: [],
    pendingReviews: [],
    recent: [],
    repairRequired: false,
    ...overrides,
  }
}

export function outcome(overrides: Partial<NodeOutcomeDto> = {}): NodeOutcomeDto {
  return { operationId: 'op-1', node: node(), cleanupPending: false, ...overrides }
}

export function gitStatus(overrides: Partial<NodeGitStatusDto> = {}): NodeGitStatusDto {
  return {
    commit: 'a'.repeat(40),
    branch: 'main',
    dirty: false,
    unfinishedOperation: false,
    ahead: 0,
    behind: 2,
    hasUpstream: true,
    status: '',
    ...overrides,
  }
}

export function registryNode(overrides: Partial<RegistryNodeDto> = {}): RegistryNodeDto {
  return {
    id: 'example-pack',
    name: 'Example Pack',
    publisher: 'example',
    description: 'Adds example nodes.',
    downloads: 1200,
    githubStars: 34,
    latestVersion: '1.2.3',
    repository: 'https://github.com/example/example-pack',
    supportedOs: ['Windows', 'Linux'],
    supportedAccelerators: ['CUDA'],
    supportedComfyuiVersion: '>=0.3.50',
    status: 'NodeStatusActive',
    os: 'compatible',
    accelerator: 'compatible',
    comfyui: 'unknown',
    flagged: false,
    deprecated: false,
    warnings: [],
    ...overrides,
  }
}

export function registryPage(
  nodes: RegistryNodeDto[] = [registryNode()],
  overrides: Partial<RegistryPageDto> = {},
): RegistryPageDto {
  return {
    nodes,
    page: 1,
    limit: 50,
    total: nodes.length,
    totalPages: 1,
    fetchedAt: '2026-10-17T08:00:00Z',
    stale: false,
    ...overrides,
  }
}

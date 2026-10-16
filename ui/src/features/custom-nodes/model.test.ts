import { describe, expect, it } from 'vitest'

import { manyNodes, node, plan, registryInstalled } from './fixtures'
import {
  canCommit,
  canRetryDependencies,
  checkGitStatus,
  chunk,
  decision,
  EMPTY_CONSENT,
  failureKind,
  filterNodes,
  formatCount,
  hasTrustNamespace,
  isUpdateCandidate,
  MAX_BATCH,
  normalizeConsent,
  repairActive,
  reviewKey,
  DEFAULT_NODE_FILTERS,
  type Consent,
} from './model'

const accepted: Consent = { ...EMPTY_CONSENT, execute: true }

describe('consent rules', () => {
  it('trusted_source_still_requires_execution_consent', () => {
    const trusted = plan({ trustedSource: true, trustedInstallPy: true, hasInstallPy: true })
    expect(canCommit(trusted, EMPTY_CONSENT)).toBe(false)
    expect(decision(trusted, EMPTY_CONSENT).executeNodeCode).toBe(false)
    expect(canCommit(trusted, accepted)).toBe(true)
    // Remembered script trust never pre-checks script execution.
    expect(decision(trusted, accepted).runInstallPy).toBe(false)
  })

  it('decision_masks_hidden_script_trust', () => {
    const everything: Consent = {
      execute: true, hook: true, warnings: true, pins: true, remember: true, rememberHook: true,
    }
    const noHook = decision(plan({ hasInstallPy: false }), everything)
    expect(noHook.runInstallPy).toBe(false)
    expect(noHook.rememberInstallPy).toBe(false)
    expect(noHook.acknowledgeWarnings).toBe(false)
    expect(noHook.confirmedChanges).toEqual([])

    const withHook = plan({ hasInstallPy: true })
    expect(decision(withHook, everything)).toMatchObject({
      runInstallPy: true, rememberSource: true, rememberInstallPy: true,
    })
    // Unchecking the script or the source trust clears the remembered script permission.
    expect(normalizeConsent(withHook, { ...everything, hook: false }).rememberHook).toBe(false)
    expect(normalizeConsent(withHook, { ...everything, remember: false }).rememberHook).toBe(false)
    expect(decision(withHook, { ...everything, remember: false })).toMatchObject({
      rememberSource: false, rememberInstallPy: false,
    })
  })

  it('registry_trust_requires_publisher_and_repository', () => {
    const registry = (publisher: string | null, repository: string | null) => ({
      kind: 'registry' as const, id: 'pack', publisher, version: '1.0.0', repository,
    })
    expect(hasTrustNamespace(registry('example', 'https://github.com/example/pack'))).toBe(true)
    expect(hasTrustNamespace(registry(null, 'https://github.com/example/pack'))).toBe(false)
    expect(hasTrustNamespace(registry('example', null))).toBe(false)
    expect(hasTrustNamespace(registry('', 'https://github.com/example/pack'))).toBe(false)
    expect(hasTrustNamespace({ kind: 'git', repository: 'https://github.com/a/b', commit: 'a', branch: null })).toBe(true)
    expect(hasTrustNamespace({ kind: 'manual' })).toBe(false)

    const untrusted = plan({ source: registry(null, null) })
    expect(decision(untrusted, { ...accepted, remember: true }).rememberSource).toBe(false)
  })

  it('decision_echoes_exact_binding_and_pin_changes', () => {
    const changes = [{ name: 'torch', from: '2.14.1', to: '2.15.0' }]
    const reviewed = plan({ id: 'p', binding: 'binding-2', protectedChanges: changes, warnings: ['Flagged'] })
    expect(reviewKey(reviewed)).not.toBe(reviewKey({ ...reviewed, binding: 'binding-3' }))
    expect(canCommit(reviewed, accepted)).toBe(false)
    expect(canCommit(reviewed, { ...accepted, warnings: true })).toBe(false)
    const consent = { ...accepted, warnings: true, pins: true }
    expect(canCommit(reviewed, consent)).toBe(true)
    expect(decision(reviewed, consent)).toMatchObject({
      planId: 'p', binding: 'binding-2', acknowledgeWarnings: true, confirmedChanges: changes,
    })
    expect(decision(reviewed, { ...consent, pins: false }).confirmedChanges).toEqual([])
  })

  it('retry_includes_not_run_directory_nodes', () => {
    expect(canRetryDependencies(node({ dependencies: 'not_run', source: { kind: 'manual' } }))).toBe(true)
    expect(canRetryDependencies(node({ dependencies: 'failed' }))).toBe(true)
    expect(canRetryDependencies(node({ dependencies: 'ok' }))).toBe(false)
    expect(canRetryDependencies(node({ dependencies: 'failed', singleFile: true }))).toBe(false)
    expect(canRetryDependencies(node({ dependencies: 'failed', conflict: true }))).toBe(false)
  })
})

describe('eligibility and lists', () => {
  it('excludes manual, single-file, conflicting and known-dirty nodes from updates', () => {
    expect(isUpdateCandidate(node())).toBe(true)
    expect(isUpdateCandidate(node({ dirty: null }))).toBe(true)
    expect(isUpdateCandidate(registryInstalled())).toBe(true)
    expect(isUpdateCandidate(node({ source: { kind: 'manual' } }))).toBe(false)
    expect(isUpdateCandidate(node({ singleFile: true }))).toBe(false)
    expect(isUpdateCandidate(node({ conflict: true }))).toBe(false)
    expect(isUpdateCandidate(node({ dirty: true }))).toBe(false)
  })

  it('refuses dirty, unfinished and diverged repositories and asks for a target when detached', () => {
    const status = {
      commit: 'a'.repeat(40), branch: 'main', dirty: false, unfinishedOperation: false,
      ahead: 0, behind: 1, hasUpstream: true, status: '',
    }
    expect(checkGitStatus(status)).toEqual({ refusals: [], needsReference: false })
    expect(checkGitStatus({ ...status, dirty: true }).refusals).toHaveLength(1)
    expect(checkGitStatus({ ...status, unfinishedOperation: true }).refusals).toHaveLength(1)
    expect(checkGitStatus({ ...status, ahead: 1, behind: 1 }).refusals).toHaveLength(1)
    expect(checkGitStatus({ ...status, branch: null }).needsReference).toBe(true)
    expect(checkGitStatus({ ...status, hasUpstream: false, ahead: null, behind: null }).needsReference).toBe(true)
  })

  it('filters by text, source and state and includes disabled nodes by default', () => {
    const nodes = [
      node({ id: 'a', name: 'Alpha' }),
      node({ id: 'b', name: 'Beta', enabled: false }),
      registryInstalled({ id: 'c', name: 'Gamma' }),
    ]
    expect(filterNodes(nodes, DEFAULT_NODE_FILTERS).map((n) => n.id)).toEqual(['a', 'b', 'c'])
    expect(filterNodes(nodes, { ...DEFAULT_NODE_FILTERS, enabled: 'disabled' }).map((n) => n.id)).toEqual(['b'])
    expect(filterNodes(nodes, { ...DEFAULT_NODE_FILTERS, source: 'registry' }).map((n) => n.id)).toEqual(['c'])
    expect(filterNodes(nodes, { ...DEFAULT_NODE_FILTERS, text: 'ALP' }).map((n) => n.id)).toEqual(['a'])
  })

  it('keeps the active identity or falls back to the first row', () => {
    const nodes = manyNodes(3)
    expect(repairActive(nodes, nodes[1].id)).toBe(nodes[1].id)
    expect(repairActive(nodes, 'gone')).toBe(nodes[0].id)
    expect(repairActive(nodes, null)).toBe(nodes[0].id)
    expect(repairActive([], 'gone')).toBeNull()
  })

  it('splits targets into groups of at most 128', () => {
    const groups = chunk(manyNodes(300), MAX_BATCH)
    expect(groups.map((group) => group.length)).toEqual([128, 128, 44])
  })

  it('maps IPC codes to the documented behaviour and keeps zero distinct from unknown', () => {
    expect(failureKind('comfyui_running')).toBe('comfyui_running')
    expect(failureKind('node_operation_running')).toBe('busy')
    expect(failureKind('node_rescan_failed')).toBe('recovery')
    expect(failureKind('registry_refused')).toBe('registry_refused')
    expect(failureKind('something_new')).toBe('other')
    expect(formatCount(0)).toBe('0')
    expect(formatCount(null)).toBe('—')
  })
})

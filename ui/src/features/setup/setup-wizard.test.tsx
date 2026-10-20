// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { gateway, type SetupModeDto } from '@/shared/ipc/gateway'
import { profile, setupStatus } from '@/shared/testing/environment'
import { SetupWizard } from './setup-wizard'

vi.mock('@/shared/ui/log-view', () => ({ LogView: () => <div>Live log</div> }))
vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    setupDetectHardware: vi.fn(),
    setupListProfiles: vi.fn(),
    setupListCuratedNodes: vi.fn(),
    setupValidatePython: vi.fn(),
    setupSelectRoot: vi.fn(),
    setupRestart: vi.fn(),
    setupRun: vi.fn(),
    setupCancel: vi.fn(),
    pickFolder: vi.fn(),
    pickFile: vi.fn(),
  },
}))

function mount(status = setupStatus, mode: SetupModeDto = 'first_run') {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  })
  return { ...render(
    <QueryClientProvider client={client}>
      <SetupWizard
        status={status}
        mode={mode}
        onClose={vi.fn()}
        refresh={vi.fn()}
        renderCuratedReview={() => null}
      />
    </QueryClientProvider>,
  ), client }
}
function next() {
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
}
async function toPython() {
  next()
  await screen.findByText('Detected')
  next()
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.setupDetectHardware).mockResolvedValue({
    gpuStatus: 'detected',
    gpus: [],
    driverVersion: '591',
    driverCuda: '13.1',
    os: 'windows',
    osVersion: '11',
    installVolumeFreeBytes: 100 * 1024 ** 3,
    envVolumeFreeBytes: null,
    envOnSeparateVolume: false,
    longPathsEnabled: true,
  })
  vi.mocked(gateway.setupListProfiles).mockResolvedValue([profile])
  vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([])
})
afterEach(cleanup)

describe('setup wizard', () => {
  it('saves a changed root and blocks setup until restart', async () => {
    vi.mocked(gateway.setupSelectRoot).mockResolvedValue({
      restartRequired: true,
    })
    vi.mocked(gateway.setupRestart).mockResolvedValue(null)
    mount()
    fireEvent.change(screen.getByLabelText('Install root'), {
      target: { value: 'E:/Comfy' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save install root' }))
    await screen.findByText('Restart required')
    expect(gateway.setupSelectRoot).toHaveBeenCalledWith(
      'E:/Comfy',
      expect.stringContaining('"version":1'),
    )
    expect(
      screen.getByRole('button', { name: 'Save install root' }),
    ).toHaveProperty('disabled', true)
    expect(gateway.setupRun).not.toHaveBeenCalled()
    expect(gateway.setupRestart).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Restart now' }))
    const restarting = await screen.findByRole('button', {
      name: 'Restarting…',
    })
    expect(restarting).toHaveProperty('disabled', true)
    expect(gateway.setupRestart).toHaveBeenCalledTimes(1)
  })
  it('disables incompatible profiles and shows their reasons', async () => {
    vi.mocked(gateway.setupListProfiles).mockResolvedValue([
      {
        ...profile,
        enabled: false,
        reasons: [
          {
            code: 'driver',
            message: 'Driver CUDA is too old for this profile',
          },
        ],
      },
    ])
    mount()
    await toPython()
    next()
    await screen.findByText('Driver CUDA is too old for this profile')
    expect(screen.getByRole('radio')).toHaveProperty('disabled', true)
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
  })
  it('requires validation and exact adopted-path consent; editing invalidates it', async () => {
    vi.mocked(gateway.setupValidatePython).mockResolvedValue({
      target: 'existing_venv',
      resolvedPath: 'D:/external',
      pythonVersion: '3.13.16',
      usable: true,
      checks: [{ id: 'arch', passed: true, detail: '64 bit', blocks: [] }],
    })
    mount()
    await toPython()
    fireEvent.click(screen.getByRole('radio', { name: 'Custom venv' }))
    fireEvent.change(screen.getByLabelText('Venv folder'), {
      target: { value: 'D:/external' },
    })
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Validate Python' }))
    await screen.findByText('Adopted environment consent')
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
    fireEvent.click(screen.getByRole('checkbox'))
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      false,
    )
    fireEvent.change(screen.getByLabelText('Venv folder'), {
      target: { value: 'D:/another' },
    })
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty(
      'disabled',
      true,
    )
  })
  it('starts a typed new run with the selected tag and the default Manager choice', async () => {
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    fireEvent.click(screen.getByRole('radio', { name: 'Specific tag' }))
    fireEvent.change(screen.getByLabelText('ComfyUI tag'), {
      target: { value: 'v0.9.0' },
    })
    next()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() =>
      expect(gateway.setupRun).toHaveBeenCalledWith(
        {
          kind: 'new',
          request: {
            mode: 'first_run',
            pythonSource: { kind: 'managed' },
            profileId: 'default',
            comfyuiRef: { kind: 'tag', value: 'v0.9.0' },
            installManager: true,
            accelerators: [],
            curatedNodes: [],
            cleanCache: false,
            confirmAdoptedChanges: false,
            allowSourceBuilds: false,
            thirdPartyConsent: true,
          },
        },
        expect.any(Function),
      ),
    )
  })

  it('latest_commit_choice_sends_the_master_branch', async () => {
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(
      screen.getByRole('radio', { name: 'Latest release' }).getAttribute('aria-checked'),
    ).toBe('true')
    expect(screen.queryByText(/development code, may be less stable/)).toBeNull()
    fireEvent.click(screen.getByRole('radio', { name: 'Latest commit (master)' }))
    expect(screen.getByText(/development code, may be less stable/)).toBeTruthy()
    expect(screen.getByText(/The checkout tracks origin\/master/)).toBeTruthy()
    expect(screen.queryByLabelText('ComfyUI tag')).toBeNull()
    next()
    expect(screen.getByText('Latest commit on master')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() =>
      expect(gateway.setupRun).toHaveBeenCalledWith(
        {
          kind: 'new',
          request: {
            mode: 'first_run',
            pythonSource: { kind: 'managed' },
            profileId: 'default',
            comfyuiRef: { kind: 'branch', value: 'master' },
            installManager: true,
            accelerators: [],
            curatedNodes: [],
            cleanCache: false,
            confirmAdoptedChanges: false,
            allowSourceBuilds: false,
            thirdPartyConsent: true,
          },
        },
        expect.any(Function),
      ),
    )
  })

  it('legacy_draft_use_tag_still_selects_specific_tag', async () => {
    mount({
      ...setupStatus,
      draft: JSON.stringify({ version: 1, useTag: true, tag: 'v0.8.0' }),
    })
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(
      screen.getByRole('radio', { name: 'Specific tag' }).getAttribute('aria-checked'),
    ).toBe('true')
    expect(screen.getByLabelText('ComfyUI tag')).toHaveProperty('value', 'v0.8.0')
  })

  it('recommended_profile_is_listed_first', async () => {
    vi.mocked(gateway.setupListProfiles).mockResolvedValue([
      { ...profile, id: 'older', label: 'Older profile', recommended: false },
      { ...profile, id: 'newer', label: 'Newer profile', recommended: true },
      { ...profile, id: 'third', label: 'Third profile', recommended: false },
    ])
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Newer profile/ })
    const radios = screen.getAllByRole('radio')
    expect(radios.map((radio) => radio.getAttribute('id'))).toEqual([
      'profile-newer',
      'profile-older',
      'profile-third',
    ])
    expect(radios[0].getAttribute('aria-checked')).toBe('true')
  })

  it('profile_change_resets_accelerator_selection', async () => {
    const option = { id: 'fast', name: 'Fast attention (experimental)', defaultSelected: true, provenance: 'Authored provenance; not app validated.' }
    const profiles = [
      { ...profile, accelerators: [option] },
      { ...profile, id: 'alternate', label: 'Alternate profile', recommended: false,
        accelerators: [{ ...option, id: 'other', name: 'Other accelerator' }] },
    ]
    vi.mocked(gateway.setupListProfiles).mockResolvedValue(profiles)
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(screen.getByRole('checkbox', { name: option.name }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText(option.provenance)).toBeTruthy()
    fireEvent.click(screen.getByRole('checkbox', { name: option.name }))
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check hardware again' }))
    await waitFor(() => expect(gateway.setupListProfiles).toHaveBeenCalledTimes(2))
    next()
    next()
    next()
    expect(screen.getByRole('checkbox', { name: option.name }).getAttribute('aria-checked')).toBe('false')
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('radio', { name: 'Alternate profile' }))
    next()
    expect(screen.getByRole('checkbox', { name: 'Other accelerator' }).getAttribute('aria-checked')).toBe('true')
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    fireEvent.click(screen.getByRole('radio', { name: /Python 3.13/ }))
    next()
    expect(screen.getByRole('checkbox', { name: option.name }).getAttribute('aria-checked')).toBe('true')
  })

  it('empty_accelerator_selection_remains_empty', async () => {
    vi.mocked(gateway.setupListProfiles).mockResolvedValue([{ ...profile, accelerators: [
      { id: 'fast', name: 'Fast accelerator', defaultSelected: true, provenance: 'Fixture evidence' },
    ] }])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    fireEvent.click(screen.getByRole('checkbox', { name: 'Fast accelerator' }))
    next()
    expect(screen.getAllByText('None selected').length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith({
      kind: 'new',
      request: {
        mode: 'first_run', pythonSource: { kind: 'managed' }, profileId: profile.id,
        comfyuiRef: { kind: 'latest' }, installManager: true,
        accelerators: [], curatedNodes: [], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false, thirdPartyConsent: true,
      },
    }, expect.any(Function)))
  })

  it('catalog_failure_blocks_start_and_refetch_preserves_unchecked_nodes', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://example.com/node.git', provenance: 'Fixture', unavailable: null }
    vi.mocked(gateway.setupListCuratedNodes).mockRejectedValueOnce(new Error('Catalog unavailable')).mockResolvedValue([entry])
    const view = mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    await screen.findByRole('checkbox', { name: entry.name })
    expect(screen.getByText(/latest commit of the default branch/)).toBeTruthy()
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('false')
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    await act(() => view.client.refetchQueries({ queryKey: ['setup-curated-nodes'] }))
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('true')
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    await act(() => view.client.refetchQueries({ queryKey: ['setup-curated-nodes'] }))
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('false')
    next()
    expect(screen.getAllByText('None selected')).toHaveLength(2)
  })

  it('unavailable_curated_nodes_start_unticked_disabled_show_reason_and_are_not_sent', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://example.com/node.git', provenance: 'Fixture', unavailable: null }
    const blocked = { ...entry, id: 'blocked', name: 'Blocked fixture', unavailable: 'Its repository tracks Python cache files' }
    vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([entry, blocked])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    await screen.findByRole('checkbox', { name: entry.name })
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('false')
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('true')
    const box = screen.getByRole('checkbox', { name: blocked.name })
    expect(box.getAttribute('aria-checked')).toBe('false')
    expect(box).toHaveProperty('disabled', true)
    expect(screen.getByText(`Unavailable: ${blocked.unavailable}`)).toBeTruthy()
    fireEvent.click(box)
    expect(screen.getByRole('checkbox', { name: blocked.name }).getAttribute('aria-checked')).toBe('false')
    next()
    expect(screen.getByText(entry.name)).toBeTruthy()
    expect(screen.queryByText(blocked.name)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith({
      kind: 'new',
      request: {
        mode: 'first_run', pythonSource: { kind: 'managed' }, profileId: profile.id,
        comfyuiRef: { kind: 'latest' }, installManager: true,
        accelerators: [], curatedNodes: ['node'], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false, thirdPartyConsent: true,
      },
    }, expect.any(Function)))
  })

  it('plaguekind_is_selectable_and_sent_with_review_consent', async () => {
    const entry = {
      id: 'comfyui-plaguekind-nodes',
      name: 'ComfyUI-PlagueKind-Nodes',
      repository: 'https://github.com/PlagueKind/ComfyUI-PlagueKind-Nodes.git',
      provenance: 'Fixture',
      unavailable: null,
    }
    vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([entry])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    const box = await screen.findByRole('checkbox', { name: entry.name })
    expect(box).toHaveProperty('disabled', false)
    expect(box.getAttribute('aria-checked')).toBe('false')
    expect(screen.getByText(/For selected custom nodes, setup allows shipped/)).toBeTruthy()
    fireEvent.click(box)
    expect(box.getAttribute('aria-checked')).toBe('true')
    next()
    expect(within(screen.getByRole('list', { name: 'Third-party sources' }))
      .getByText(entry.repository)).toBeTruthy()
    expect(screen.getByText(/runs install.py if present, then installs the reviewed requirements.txt/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith({
      kind: 'new',
      request: {
        mode: 'first_run', pythonSource: { kind: 'managed' }, profileId: profile.id,
        comfyuiRef: { kind: 'latest' }, installManager: true,
        accelerators: [], curatedNodes: [entry.id], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false, thirdPartyConsent: true,
      },
    }, expect.any(Function)))
  })

  it('review_page_discloses_third_party_code_and_sends_consent', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://github.com/example/node.git', provenance: 'Fixture', unavailable: null }
    const other = { id: 'other', name: 'Other fixture', repository: 'https://github.com/example/other.git', provenance: 'Fixture', unavailable: null }
    const activeEnv = { id: 'selected', path: 'D:/Comfy/env', ownership: 'app_owned' as const, source: 'managed' as const, pythonVersion: profile.python, profileId: profile.id, profileRevision: 1, state: 'verified' as const, pins: [], verification: null }
    const disclosure = "Installing ComfyUI-Manager and selected custom nodes runs third-party code on this computer; a Python environment is not a sandbox. Setup installs the latest commit of each repository’s default branch. For selected custom nodes, setup allows shipped Python cache files and top-level native programs or libraries, runs install.py if present, then installs the reviewed requirements.txt. It does not ask again. Clicking Start setup confirms this."
    const managerSource = 'https://github.com/Comfy-Org/ComfyUI-Manager'
    vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([entry, other])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    const sources = () => within(screen.getByRole('list', { name: 'Third-party sources' }))
      .getAllByRole('listitem').map((item) => item.textContent)
    const toOptions = async () => {
      await toPython()
      next()
      await screen.findByRole('radio', { name: /Python 3.13/ })
      next()
      await screen.findByRole('checkbox', { name: entry.name })
    }
    const startWith = async (request: Record<string, unknown>) => {
      fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
      await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(1))
      const [action] = vi.mocked(gateway.setupRun).mock.calls[0]
      expect(action).toMatchObject({ kind: 'new', request })
    }
    const again = () => {
      cleanup()
      vi.mocked(gateway.setupRun).mockClear()
    }

    // Selected curated nodes are listed and covered by the click, after the Manager source.
    mount()
    await toOptions()
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    fireEvent.click(screen.getByRole('checkbox', { name: other.name }))
    next()
    expect(screen.getByText(disclosure)).toBeTruthy()
    expect(sources()).toEqual([managerSource, entry.repository, other.repository])
    await startWith({ installManager: true, curatedNodes: ['node', 'other'], thirdPartyConsent: true })

    // Manager alone (the default) is enough, and its display source comes first.
    again()
    mount()
    await toOptions()
    next()
    expect(screen.getByText(disclosure)).toBeTruthy()
    expect(sources()).toEqual([managerSource])
    await startWith({ installManager: true, curatedNodes: [], thirdPartyConsent: true })
    again()
    mount()
    await toOptions()
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    next()
    expect(sources()).toEqual([managerSource, entry.repository])

    // Curated nodes alone are enough without the Manager.
    again()
    mount()
    await toOptions()
    fireEvent.click(screen.getByRole('checkbox', { name: /Install ComfyUI-Manager/ }))
    fireEvent.click(screen.getByRole('checkbox', { name: other.name }))
    next()
    expect(sources()).toEqual([other.repository])
    await startWith({ installManager: false, curatedNodes: ['other'], thirdPartyConsent: true })

    // Nothing third-party selected: no disclosure and no consent.
    again()
    mount()
    await toOptions()
    fireEvent.click(screen.getByRole('checkbox', { name: /Install ComfyUI-Manager/ }))
    next()
    expect(screen.queryByText(disclosure)).toBeNull()
    expect(screen.queryByRole('list', { name: 'Third-party sources' })).toBeNull()
    await startWith({ installManager: false, curatedNodes: [], thirdPartyConsent: false })

    // Other modes keep their per-install prompts: Manager never carries the consent.
    again()
    mount({ ...setupStatus, activeEnv }, 'change_torch')
    await screen.findByText('Detected')
    next()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    fireEvent.click(screen.getByRole('checkbox', { name: /Install ComfyUI-Manager/ }))
    next()
    expect(screen.getByText(/Manager selected; consent still required/)).toBeTruthy()
    expect(screen.queryByText(disclosure)).toBeNull()
    await startWith({ mode: 'change_torch', installManager: true, curatedNodes: [], thirdPartyConsent: false })
    again()
    mount({ ...setupStatus, activeEnv }, 'repair')
    await screen.findByText('Detected')
    next()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    next()
    expect(screen.queryByText(disclosure)).toBeNull()
    await startWith({ mode: 'repair', curatedNodes: [], thirdPartyConsent: false })
  })

  it('first_run_ticks_comfyui_manager_and_no_curated_nodes_by_default', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://github.com/example/node.git', provenance: 'Fixture', unavailable: null }
    const other = { id: 'other', name: 'Other fixture', repository: 'https://github.com/example/other.git', provenance: 'Fixture', unavailable: null }
    const blocked = { id: 'blocked', name: 'Blocked fixture', repository: 'https://github.com/example/blocked.git', provenance: 'Fixture', unavailable: 'Its repository tracks Python cache files' }
    vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([entry, other, blocked])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    await screen.findByRole('checkbox', { name: entry.name })
    expect(screen.getByRole('checkbox', { name: /Install ComfyUI-Manager/ }).getAttribute('aria-checked')).toBe('true')
    for (const node of [entry, other, blocked])
      expect(screen.getByRole('checkbox', { name: node.name }).getAttribute('aria-checked')).toBe('false')
    next()
    expect(within(screen.getByRole('list', { name: 'Third-party sources' }))
      .getAllByRole('listitem').map((item) => item.textContent)).toEqual(['https://github.com/Comfy-Org/ComfyUI-Manager'])
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledWith({
      kind: 'new',
      request: {
        mode: 'first_run', pythonSource: { kind: 'managed' }, profileId: profile.id,
        comfyuiRef: { kind: 'latest' }, installManager: true,
        accelerators: [], curatedNodes: [], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false, thirdPartyConsent: true,
      },
    }, expect.any(Function)))
  })

  it('other_modes_leave_comfyui_manager_unticked', async () => {
    const activeEnv = { id: 'selected', path: 'D:/Comfy/env', ownership: 'app_owned' as const, source: 'managed' as const, pythonVersion: profile.python, profileId: profile.id, profileRevision: 1, state: 'verified' as const, pins: [], verification: null }
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount({ ...setupStatus, activeEnv }, 'change_torch')
    await screen.findByText('Detected')
    next()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(screen.getByRole('checkbox', { name: /Install ComfyUI-Manager/ }).getAttribute('aria-checked')).toBe('false')
    next()
    expect(screen.queryByRole('list', { name: 'Third-party sources' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Start setup' }))
    await waitFor(() => expect(gateway.setupRun).toHaveBeenCalledTimes(1))
    expect(vi.mocked(gateway.setupRun).mock.calls[0][0]).toMatchObject({
      kind: 'new',
      request: { mode: 'change_torch', installManager: false, curatedNodes: [], thirdPartyConsent: false },
    })
  })

  it('repair_retains_recorded_component_choices_without_querying_curated_catalog', async () => {
    mount({ ...setupStatus, activeEnv: { id: 'selected', path: 'D:/Comfy/env', ownership: 'app_owned', source: 'managed', pythonVersion: profile.python, profileId: profile.id, profileRevision: 1, state: 'verified', pins: [], verification: null } }, 'repair')
    await screen.findByText('Detected')
    next()
    next()
    next()
    expect(screen.getByText('Repair retains the recorded Manager and accelerator choices.')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Install ComfyUI-Manager/ })).toBeNull()
    expect(gateway.setupListCuratedNodes).not.toHaveBeenCalled()
  })
})

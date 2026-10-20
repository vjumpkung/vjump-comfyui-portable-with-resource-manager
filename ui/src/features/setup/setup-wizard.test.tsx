// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
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
  it('starts a typed new run with the selected tag and Manager off by default', async () => {
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
            installManager: false,
            accelerators: [],
            curatedNodes: [],
            cleanCache: false,
            confirmAdoptedChanges: false,
            allowSourceBuilds: false,
          },
        },
        expect.any(Function),
      ),
    )
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
        comfyuiRef: { kind: 'latest' }, installManager: false,
        accelerators: [], curatedNodes: [], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false,
      },
    }, expect.any(Function)))
  })

  it('catalog_failure_blocks_start_and_refetch_preserves_unchecked_nodes', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://example.com/node.git', branch: 'main', commit: 'a'.repeat(40), provenance: 'Fixture', unavailable: null }
    vi.mocked(gateway.setupListCuratedNodes).mockRejectedValueOnce(new Error('Catalog unavailable')).mockResolvedValue([entry])
    const view = mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    expect(screen.getByRole('button', { name: 'Continue' })).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    await screen.findByRole('checkbox', { name: entry.name })
    expect(screen.getByText(/branch main \(informational\)/)).toBeTruthy()
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('true')
    fireEvent.click(screen.getByRole('checkbox', { name: entry.name }))
    await act(() => view.client.refetchQueries({ queryKey: ['setup-curated-nodes'] }))
    expect(screen.getByRole('checkbox', { name: entry.name }).getAttribute('aria-checked')).toBe('false')
    next()
    expect(screen.getAllByText('None selected')).toHaveLength(2)
  })

  it('unavailable_curated_nodes_start_unticked_disabled_show_reason_and_are_not_sent', async () => {
    const entry = { id: 'node', name: 'Curated fixture', repository: 'https://example.com/node.git', branch: 'main', commit: 'a'.repeat(40), provenance: 'Fixture', unavailable: null }
    const blocked = { ...entry, id: 'blocked', name: 'Blocked fixture', unavailable: 'Not available at this pinned commit' }
    vi.mocked(gateway.setupListCuratedNodes).mockResolvedValue([entry, blocked])
    vi.mocked(gateway.setupRun).mockResolvedValue({ runId: 'run-new' })
    mount()
    await toPython()
    next()
    await screen.findByRole('radio', { name: /Python 3.13/ })
    next()
    await screen.findByRole('checkbox', { name: entry.name })
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
        comfyuiRef: { kind: 'latest' }, installManager: false,
        accelerators: [], curatedNodes: ['node'], cleanCache: false,
        confirmAdoptedChanges: false, allowSourceBuilds: false,
      },
    }, expect.any(Function)))
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

// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import type { CustomNodesPageProps } from '@/features/custom-nodes'
import type { SetupStatusDto, StartResultDto } from '@/shared/ipc/gateway'
import { setupStatus } from '@/shared/testing/environment'

import { AppShell } from './app-shell'

const runtimeActions = vi.hoisted(() => ({
  start: { isPending: false },
  stop: { isPending: false, mutateAsync: vi.fn() },
  restart: { isPending: false, mutateAsync: vi.fn() },
}))
const runtimeState = vi.hoisted(() => ({
  status: { lifecycle: 'running' },
}))

vi.mock('@/features/comfy-runtime', () => ({
  ComfyPage: () => <p>ComfyUI page</p>,
  CloseRequestDialog: () => null,
  RuntimeControls: () => null,
  useRuntime: () => ({ status: runtimeState.status }),
  useRuntimeActions: () => runtimeActions,
}))
vi.mock('@/features/custom-nodes', () => ({
  CustomNodesPage: (props: CustomNodesPageProps) => {
    const [result, setResult] = useState('none')
    return (
      <div>
        <p>Custom Nodes feature</p>
        <p>busy:{String(props.maintenanceBusy)} runtime:{props.runtime?.lifecycle ?? 'none'} actions:{String(props.runtimeBusy)}</p>
        <p>restart result:{result}</p>
        <button onClick={props.onRepair}>Repair stub</button>
        <button onClick={props.onOpenComfy}>Open ComfyUI stub</button>
        <button onClick={() => void props.onStopComfy().then(() => setResult('stopped'))}>Stop stub</button>
        <button onClick={() => void props.onRestartComfy().then((value) => setResult(value.kind))}>Restart stub</button>
      </div>
    )
  },
}))
vi.mock('@/features/downloads', () => ({ DownloadsPage: () => null }))
vi.mock('@/features/library', () => ({ LibraryPage: () => null }))
vi.mock('@/features/overview', () => ({ OverviewPage: () => <p>Overview page</p> }))
vi.mock('@/features/settings', () => ({ SettingsPage: () => null }))
vi.mock('@/features/environment', () => ({ EnvironmentPage: () => null }))
vi.mock('./top-bar', () => ({ TopBar: () => null }))

const onSetup = vi.fn()
const onResume = vi.fn()

function mount(status: SetupStatusDto = { ...setupStatus, phase: 'ready' }) {
  render(<AppShell status={status} onSetup={onSetup} onResume={onResume} />)
  fireEvent.click(screen.getByRole('button', { name: 'Custom Nodes' }))
}

beforeEach(() => {
  vi.resetAllMocks()
  runtimeState.status = { lifecycle: 'running' }
  runtimeActions.stop.mutateAsync.mockResolvedValue(undefined)
})
afterEach(cleanup)

it('custom_nodes_navigation_renders_the_feature', () => {
  mount()
  expect(screen.getByText('Custom Nodes feature')).toBeTruthy()
  // The shell hands the feature the maintenance flag and the live runtime status.
  expect(screen.getByText('busy:false runtime:running actions:false')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Open ComfyUI stub' }))
  expect(screen.getByText('ComfyUI page')).toBeTruthy()
  expect(screen.queryByText('Custom Nodes feature')).toBeNull()
  fireEvent.click(screen.getByRole('button', { name: 'Overview' }))
  expect(screen.getByText('Overview page')).toBeTruthy()
})

it('reports_setup_in_progress_as_maintenance_busy', () => {
  mount({ ...setupStatus, phase: 'run_in_progress' })
  expect(screen.getByText('busy:true runtime:running actions:false')).toBeTruthy()
})

it('node_repair_uses_existing_setup_facade', () => {
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Repair stub' }))
  expect(onSetup).toHaveBeenCalledTimes(1)
  expect(onSetup).toHaveBeenCalledWith('repair')
})

it('node_restart_uses_runtime_action_result', async () => {
  const conflict: StartResultDto = { kind: 'port_conflict', requested: 8188, suggested: 8189 }
  runtimeActions.restart.mutateAsync.mockResolvedValue(conflict)
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Restart stub' }))
  // The feature receives the unchanged result, so a port conflict is never reported as started.
  await waitFor(() => expect(screen.getByText('restart result:port_conflict')).toBeTruthy())
  expect(runtimeActions.restart.mutateAsync).toHaveBeenCalledTimes(1)
})

it('awaits_the_stop_mutation_for_the_feature', async () => {
  let release: () => void = () => undefined
  runtimeActions.stop.mutateAsync.mockReturnValue(new Promise<void>((resolve) => { release = resolve }))
  mount()
  fireEvent.click(screen.getByRole('button', { name: 'Stop stub' }))
  await new Promise((resolve) => setTimeout(resolve, 20))
  expect(screen.getByText('restart result:none')).toBeTruthy()
  release()
  await waitFor(() => expect(screen.getByText('restart result:stopped')).toBeTruthy())
})

it('shell_contains_overflow_inside_main', () => {
  const view = render(<AppShell status={{ ...setupStatus, phase: 'ready' }} onSetup={onSetup} onResume={onResume} />)
  // A positioned, clipped shell keeps absolutely positioned descendants from extending the document.
  const root = view.container.firstElementChild as HTMLElement
  expect(root.classList.contains('overflow-hidden')).toBe(true)
  expect(root.classList.contains('relative')).toBe(true)
  const main = screen.getByRole('main')
  for (const name of ['relative', 'min-h-0', 'overflow-y-auto']) {
    expect(main.classList.contains(name)).toBe(true)
  }
})

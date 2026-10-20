// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { gateway, IpcClientError } from '@/shared/ipc/gateway'

import { ExtraModelPathsCard } from './extra-model-paths-card'

vi.mock('@/shared/ipc/gateway', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/ipc/gateway')>()),
  gateway: {
    comfyReadExtraModelPaths: vi.fn(),
    comfyGenerateExtraModelPaths: vi.fn(),
    comfyRemoveExtraModelPaths: vi.fn(),
    pickFolder: vi.fn(),
  },
}))

const appRoot = {
  section: 'vjumpkung_desktop',
  category: 'checkpoints',
  path: 'D:\\Models\\checkpoints',
  isDefault: false,
}
const handWritten = {
  section: 'legacy',
  category: 'loras',
  path: 'D:\\legacy\\loras',
  isDefault: false,
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(gateway.comfyReadExtraModelPaths).mockResolvedValue([])
})
afterEach(cleanup)

function card(running = false) {
  return (
    <QueryClientProvider client={new QueryClient()}>
      <ExtraModelPathsCard running={running} />
    </QueryClientProvider>
  )
}

function removeButton() {
  return screen.getByRole<HTMLButtonElement>('button', {
    name: 'Remove generated section',
  })
}

it('generates_with_the_chosen_folder_and_refreshes_the_list', async () => {
  vi.mocked(gateway.comfyReadExtraModelPaths)
    .mockResolvedValueOnce([])
    .mockResolvedValue([appRoot])
  vi.mocked(gateway.comfyGenerateExtraModelPaths).mockResolvedValue(null)
  vi.mocked(gateway.pickFolder).mockResolvedValue('D:\\Models')
  render(card())
  await screen.findByText('No extra model paths are configured.')
  const generate = screen.getByRole<HTMLButtonElement>('button', {
    name: 'Generate',
  })
  expect(generate.disabled).toBe(true)

  fireEvent.click(screen.getByRole('button', { name: 'Browse' }))
  await waitFor(() =>
    expect(
      screen.getByLabelText<HTMLInputElement>('Model base folder').value,
    ).toBe('D:\\Models'),
  )
  expect(gateway.pickFolder).toHaveBeenCalledTimes(1)

  fireEvent.change(screen.getByLabelText('Model base folder'), {
    target: { value: '  D:\\Models  ' },
  })
  fireEvent.click(generate)
  await waitFor(() =>
    expect(gateway.comfyGenerateExtraModelPaths).toHaveBeenCalledWith(
      'D:\\Models',
    ),
  )
  expect(await screen.findByText('D:\\Models\\checkpoints')).toBeTruthy()
  expect(gateway.comfyReadExtraModelPaths).toHaveBeenCalledTimes(2)
})

it('remove_is_offered_only_for_the_app_section', async () => {
  vi.mocked(gateway.comfyReadExtraModelPaths).mockResolvedValue([handWritten])
  vi.mocked(gateway.comfyRemoveExtraModelPaths).mockResolvedValue(null)
  const first = render(card())
  await screen.findByText('D:\\legacy\\loras')
  expect(removeButton().disabled).toBe(true)
  first.unmount()

  vi.mocked(gateway.comfyReadExtraModelPaths).mockResolvedValue([
    handWritten,
    appRoot,
  ])
  render(card())
  await screen.findByText('D:\\Models\\checkpoints')
  await waitFor(() => expect(removeButton().disabled).toBe(false))
  fireEvent.click(removeButton())
  await waitFor(() =>
    expect(gateway.comfyRemoveExtraModelPaths).toHaveBeenCalledTimes(1),
  )
})

it('shows_restart_hint_while_comfyui_runs', async () => {
  vi.mocked(gateway.comfyGenerateExtraModelPaths).mockResolvedValue(null)
  const client = new QueryClient()
  const page = (running: boolean) => (
    <QueryClientProvider client={client}>
      <ExtraModelPathsCard running={running} />
    </QueryClientProvider>
  )
  const view = render(page(true))
  await screen.findByText('No extra model paths are configured.')
  expect(screen.queryByText('Restart ComfyUI to use the new paths.')).toBeNull()

  fireEvent.change(screen.getByLabelText('Model base folder'), {
    target: { value: 'D:\\Models' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Generate' }))
  expect(
    await screen.findByText('Restart ComfyUI to use the new paths.'),
  ).toBeTruthy()

  // The hint belongs to the running session that has not seen the change.
  view.rerender(page(false))
  expect(screen.queryByText('Restart ComfyUI to use the new paths.')).toBeNull()
  view.rerender(page(true))
  expect(screen.queryByText('Restart ComfyUI to use the new paths.')).toBeNull()
})

it('shows_the_backend_message_when_a_folder_is_refused', async () => {
  vi.mocked(gateway.comfyGenerateExtraModelPaths).mockRejectedValue(
    new IpcClientError(
      'extra_model_paths_invalid',
      'Choose an absolute folder path on a local drive or network share.',
    ),
  )
  render(card())
  await screen.findByText('No extra model paths are configured.')
  fireEvent.change(screen.getByLabelText('Model base folder'), {
    target: { value: 'models' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Generate' }))
  expect((await screen.findByRole('alert')).textContent).toBe(
    'Choose an absolute folder path on a local drive or network share.',
  )
})

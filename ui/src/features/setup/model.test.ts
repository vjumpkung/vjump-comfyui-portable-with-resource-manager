import { describe, expect, it } from 'vitest'
import { environment, profile, run } from '@/shared/testing/environment'
import { canSkip, profileReasons, readDraft } from './model'

describe('setup policies', () => {
  it('preserves backend eligibility reasons and enforces mode-specific Python minors', () => {
    expect(
      profileReasons(
        {
          ...profile,
          enabled: false,
          reasons: [{ code: 'driver', message: 'Driver too old' }],
        },
        'first_run',
        null,
      ),
    ).toEqual(['Driver too old'])
    expect(profileReasons(profile, 'change_python', environment)).toContain(
      'Choose a different Python minor version for this mode.',
    )
    expect(
      profileReasons(
        { ...profile, python: '3.12.12' },
        'change_torch',
        environment,
      ),
    ).toHaveLength(1)
  })
  it('allows skip only for the exact failed optional step, never required or best effort', () => {
    expect(canSkip(run)).toBe(true)
    expect(canSkip({ ...run, status: 'running' })).toBe(false)
    expect(canSkip({ ...run, failure: { ...run.failure!, canSkip: false } })).toBe(false)
    for (const policy of ['required', 'best_effort'] as const)
      expect(
        canSkip({
          ...run,
          steps: run.steps.map((step) => ({ ...step, policy })),
        }),
      ).toBe(false)
    expect(
      canSkip({ ...run, failure: { ...run.failure!, step: 'torch' } }),
    ).toBe(false)
  })
  it('restores only versioned draft fields and never restores consent', () => {
    expect(readDraft('{broken')).toEqual({})
    expect(readDraft('{"version":2,"profileId":"x"}')).toEqual({})
    expect(
      readDraft(
        JSON.stringify({
          version: 1,
          pythonSource: { kind: 'customVenv', path: 'D:/venv' },
          confirmAdoptedChanges: true,
          installManager: false,
        }),
      ),
    ).toEqual({
      pythonSource: { kind: 'customVenv', path: 'D:/venv' },
      installManager: false,
    })
  })
})

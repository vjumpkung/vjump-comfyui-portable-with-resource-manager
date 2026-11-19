import { fireEvent, screen } from '@testing-library/react'

/** Opens a Radix Select the way a keyboard user does. */
export function openSelect(trigger: HTMLElement) {
  fireEvent.keyDown(trigger, { key: 'ArrowDown' })
}

/** Opens the select and picks the option with the given accessible name. */
export async function chooseOption(trigger: HTMLElement, name: string | RegExp) {
  openSelect(trigger)
  fireEvent.click(await screen.findByRole('option', { name }))
}

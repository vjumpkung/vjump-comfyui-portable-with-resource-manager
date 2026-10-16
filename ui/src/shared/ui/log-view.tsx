import { useEffect, useRef } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'

/** Logs arrive redacted by the backend. Strip terminal controls as well so subprocess
 * output cannot add links, change the title, or issue terminal escape commands. */
export function plainLog(line: string): string {
  return (
    line
      .replace(
        // eslint-disable-next-line no-control-regex -- terminal controls are intentionally removed
        /\x1b(?:\][^\x07\x1b]*(?:\x07|\x1b\\)|\[[0-?]*[ -/]*[@-~]|.)/g,
        '',
      )
      // eslint-disable-next-line no-control-regex -- exclude nonprinting control bytes from terminal output
      .replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, '')
  )
}

function newTerminal() {
  return new Terminal({
    disableStdin: true,
    convertEol: true,
    scrollback: 1500,
    rows: 12,
    fontFamily: '"Geist Mono Variable", "Noto Sans Thai Looped", monospace',
    fontSize: 12,
    screenReaderMode: true,
    theme: {
      background: '#2a313a',
      foreground: '#e0e5ec',
      cyan: '#9ffaff',
      blue: '#9ffaff',
      yellow: '#fff89c',
      cursor: '#ff9fed',
      selectionBackground: '#ff9fed55',
    },
  })
}

export function LogView({ lines }: { lines: string[] }) {
  const host = useRef<HTMLDivElement>(null)
  const terminal = useRef<Terminal | null>(null)
  useEffect(() => {
    if (!host.current) return
    const term = newTerminal()
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host.current)
    terminal.current = term
    const observer = new ResizeObserver(() => {
      if (host.current?.clientWidth) fit.fit()
    })
    observer.observe(host.current)
    return () => {
      observer.disconnect()
      terminal.current = null
      term.dispose()
    }
  }, [])
  useEffect(() => {
    const term = terminal.current
    if (!term) return
    term.clear()
    // Bounded React buffer also bounds terminal work on long installations.
    term.write('\x1b[H\x1b[2J')
    term.write(lines.map(plainLog).join('\r\n'))
  }, [lines])
  return (
    <div
      ref={host}
      aria-label="Sanitized setup log"
      className="setup-log h-72 min-w-0 rounded-lg border border-secondary p-3"
    />
  )
}

/** Streams raw output into a terminal, preserving carriage returns so progress bars redraw in
 * place. `text` only grows by appending; any other change rewrites the terminal. */
export function StreamLogView({
  text,
  label,
}: {
  text: string
  label: string
}) {
  const host = useRef<HTMLDivElement>(null)
  const terminal = useRef<Terminal | null>(null)
  const written = useRef('')
  useEffect(() => {
    if (!host.current) return
    const term = newTerminal()
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host.current)
    terminal.current = term
    written.current = ''
    const observer = new ResizeObserver(() => {
      if (host.current?.clientWidth) fit.fit()
    })
    observer.observe(host.current)
    return () => {
      observer.disconnect()
      terminal.current = null
      term.dispose()
    }
  }, [])
  useEffect(() => {
    const term = terminal.current
    if (!term) return
    if (text.startsWith(written.current)) {
      term.write(text.slice(written.current.length))
    } else {
      term.reset()
      term.write(text)
    }
    written.current = text
  }, [text])
  return (
    <div
      ref={host}
      aria-label={label}
      className="setup-log h-80 min-w-0 rounded-lg border border-secondary p-3"
    />
  )
}

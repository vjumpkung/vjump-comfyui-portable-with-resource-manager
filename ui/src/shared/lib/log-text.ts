/** Removes terminal commands and nonprinting bytes from subprocess output but keeps the
 * carriage returns and line feeds that carry progress-bar semantics (tqdm rewrites a line
 * with `\r`). Used for streamed ComfyUI output, which the backend has already redacted. */
export function plainStream(text: string): string {
  return (
    text
      .replace(
        // eslint-disable-next-line no-control-regex -- terminal controls are intentionally removed
        /\x1b(?:\][^\x07\x1b]*(?:\x07|\x1b\\)|\[[0-?]*[ -/]*[@-~]|.)/g,
        '',
      )
      // eslint-disable-next-line no-control-regex -- exclude nonprinting control bytes, keeping \t \n \r
      .replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g, '')
  )
}

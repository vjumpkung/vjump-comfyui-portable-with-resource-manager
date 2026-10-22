import { expect, it } from 'vitest'

import { allowedUrl, BLOCKED_URL } from './model-viewer'

const prefix = 'http://gallery.localhost/output/'

it('lets a model load its own embedded data and files of its own area', () => {
  expect(allowedUrl('blob:http://tauri.localhost/1b2c', prefix)).toBe('blob:http://tauri.localhost/1b2c')
  expect(allowedUrl('data:image/png;base64,AAAA', prefix)).toBe('data:image/png;base64,AAAA')
  expect(allowedUrl(`${prefix}models/robot.gltf?v=r1`, prefix)).toBe(`${prefix}models/robot.gltf?v=r1`)
  expect(allowedUrl(`${prefix}models/my tex.png`, prefix)).toBe(`${prefix}models/my%20tex.png`)
})

it('replaces everything else with a URL that cannot be decoded', () => {
  for (const url of [
    'https://example.com/tex.png',
    'http://gallery.localhost/input/tex.png',
    'http://gallery.localhost.example.com/output/tex.png',
    'file:///C:/Windows/win.ini',
    'javascript:alert(1)',
    'tex.png',
    '',
    // `..` climbs out of the area before the request is made, so it is judged after resolving.
    `${prefix}../input/tex.png`,
    `${prefix}models/%2e%2e/../../input/tex.png`,
  ]) {
    expect(allowedUrl(url, prefix), url).toBe(BLOCKED_URL)
  }
  expect(BLOCKED_URL.startsWith('data:')).toBe(true)
})

it('guards the non-Windows protocol form too', () => {
  const unix = 'gallery://localhost/input/'
  expect(allowedUrl('gallery://localhost/input/a.glb?v=1', unix)).toBe('gallery://localhost/input/a.glb?v=1')
  expect(allowedUrl('gallery://localhost/output/a.glb', unix)).toBe(BLOCKED_URL)
  expect(allowedUrl('gallery://localhost/input/../output/a.glb', unix)).toBe(BLOCKED_URL)
})

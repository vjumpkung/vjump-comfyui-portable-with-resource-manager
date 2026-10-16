import '@fontsource-variable/inter'
import '@fontsource-variable/geist-mono'
import '@fontsource/noto-sans-thai/thai-400.css'
import '@fontsource/noto-sans-thai/thai-500.css'
import '@fontsource/noto-sans-thai/thai-700.css'
import '@fontsource/noto-sans-thai-looped/thai-400.css'
import './globals.css'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { initTheme } from '@/shared/lib/theme'

import { App } from './App'

initTheme()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

import '@fontsource-variable/plus-jakarta-sans'
import '@fontsource-variable/geist-mono'
import '@fontsource/kanit/thai-400.css'
import '@fontsource/kanit/thai-500.css'
import '@fontsource/kanit/thai-600.css'
import '@fontsource/kanit/thai-700.css'
import '@fontsource/kanit/thai-800.css'
import './globals.css'

import { lazy, StrictMode, Suspense } from 'react'
import { createRoot } from 'react-dom/client'

import { initLanguage } from '@/shared/i18n'
import { initTheme } from '@/shared/lib/theme'
import { LoadingScreen } from '@/shared/ui/loading-screen'

import { BootGate } from './boot-gate'

const appImport = import('./App')
const App = lazy(() => appImport.then((module) => ({ default: module.App })))

initTheme()
initLanguage()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BootGate>
      <Suspense fallback={<LoadingScreen />}>
        <App />
      </Suspense>
    </BootGate>
  </StrictMode>,
)

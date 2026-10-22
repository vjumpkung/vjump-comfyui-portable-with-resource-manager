import {
  Boxes,
  Cpu,
  Download,
  LayoutDashboard,
  Puzzle,
  Settings,
  Workflow,
  type LucideIcon,
} from 'lucide-react'

export type NavId =
  | 'overview'
  | 'comfyui'
  | 'downloads'
  | 'models'
  | 'custom-nodes'
  | 'environment'
  | 'settings'

export interface NavItem {
  id: NavId
  labelKey: Exclude<keyof typeof import('@/shared/i18n/translations.json').en, `${string}_one` | `${string}_other`>
  icon: LucideIcon
}

export const NAV_ITEMS: readonly NavItem[] = [
  { id: 'overview', labelKey: 'nav.overview', icon: LayoutDashboard },
  { id: 'comfyui', labelKey: 'nav.comfyui', icon: Workflow },
  { id: 'downloads', labelKey: 'nav.downloads', icon: Download },
  { id: 'models', labelKey: 'nav.models', icon: Boxes },
  { id: 'custom-nodes', labelKey: 'nav.customNodes', icon: Puzzle },
  { id: 'environment', labelKey: 'nav.environment', icon: Cpu },
  { id: 'settings', labelKey: 'nav.settings', icon: Settings },
]

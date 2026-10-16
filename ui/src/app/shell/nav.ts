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
  label: string
  icon: LucideIcon
}

export const NAV_ITEMS: readonly NavItem[] = [
  { id: 'overview', label: 'Overview', icon: LayoutDashboard },
  { id: 'comfyui', label: 'ComfyUI', icon: Workflow },
  { id: 'downloads', label: 'Downloads', icon: Download },
  { id: 'models', label: 'Models', icon: Boxes },
  { id: 'custom-nodes', label: 'Custom Nodes', icon: Puzzle },
  { id: 'environment', label: 'Environment', icon: Cpu },
  { id: 'settings', label: 'Settings', icon: Settings },
]

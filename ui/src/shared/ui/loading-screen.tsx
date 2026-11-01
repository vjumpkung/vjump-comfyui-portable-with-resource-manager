import { useTranslation } from '@/shared/i18n'

export function LoadingScreen() {
  const { t } = useTranslation()
  return <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background text-foreground"
    role="status" aria-busy="true">
    <span className="h-8 w-8 animate-spin rounded-full border-4 border-muted border-t-brand-cyan motion-reduce:animate-none"
      aria-hidden="true" />
    <span className="font-sans text-lg font-semibold">vjumpkung ComfyUI</span>
    <span className="text-sm text-muted-foreground">{t('app.starting')}</span>
  </div>
}

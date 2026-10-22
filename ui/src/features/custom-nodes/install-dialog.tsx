import { useId, useState } from 'react'

import { useTranslation } from '@/shared/i18n'
import type { RegistryNodeDto } from '@/shared/ipc/gateway'
import { Alert, AlertDescription } from '@/shared/ui/alert'
import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { RadioGroup, RadioGroupItem } from '@/shared/ui/radio-group'

import type { NodePreparation } from './model'

export type InstallTarget =
  | { kind: 'registry'; node: RegistryNodeDto }
  | { kind: 'git' }

const HTTPS_URL = /^https:\/\/[^\s/]+\/\S+$/i

/** Chooses what to prepare. Preparation only produces a review; nothing is installed here. */
export function InstallDialog({ target, onPrepare, onClose }: {
  target: InstallTarget
  onPrepare: (preparation: NodePreparation) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const prefix = useId()
  const node = target.kind === 'registry' ? target.node : null
  const [source, setSource] = useState<'registry' | 'git'>(target.kind)
  const [version, setVersion] = useState('')
  const [repository, setRepository] = useState(node?.repository ?? '')
  const [reference, setReference] = useState('')
  const [touched, setTouched] = useState(false)

  const repositoryError = source === 'git' && !HTTPS_URL.test(repository.trim())
    ? t('customNodes.install.repositoryError')
    : null

  function submit() {
    setTouched(true)
    if (repositoryError !== null) return
    if (source === 'registry' && node) {
      onPrepare({ kind: 'registry', id: node.id, version: version.trim() || null })
    } else {
      onPrepare({ kind: 'git', repository: repository.trim(), reference: reference.trim() || null })
    }
  }

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{node ? t('customNodes.installName', { name: node.name }) : t('customNodes.install.gitTitle')}</DialogTitle>
          <DialogDescription>
            {t('customNodes.install.hint')}
          </DialogDescription>
        </DialogHeader>
        {node && (node.flagged || node.deprecated) && (
          <Alert>
            <AlertDescription>
              {t(node.flagged && node.deprecated ? 'customNodes.install.flaggedDeprecated'
                : node.flagged ? 'customNodes.install.flagged' : 'customNodes.install.deprecated')}
            </AlertDescription>
          </Alert>
        )}
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => { event.preventDefault(); submit() }}
        >
          {node && node.repository !== null && (
            <RadioGroup
              aria-label={t('customNodes.install.source')}
              value={source}
              onValueChange={(value) => setSource(value === 'git' ? 'git' : 'registry')}
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id={`${prefix}-registry`} value="registry" />
                <Label htmlFor={`${prefix}-registry`}>{t('customNodes.install.registryRelease')}</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id={`${prefix}-git`} value="git" />
                <Label htmlFor={`${prefix}-git`}>{t('customNodes.install.gitRepository')}</Label>
              </div>
            </RadioGroup>
          )}
          {source === 'registry' && node ? (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={`${prefix}-version`}>{t('customNodes.install.version')}</Label>
              <Input
                id={`${prefix}-version`}
                className="font-mono"
                placeholder={node.latestVersion ?? t('setup.latestRelease')}
                value={version}
                onChange={(event) => setVersion(event.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                {t('customNodes.install.latestHint')}
              </p>
            </div>
          ) : (
            <>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={`${prefix}-repository`}>{t('customNodes.install.repositoryUrl')}</Label>
                <Input
                  id={`${prefix}-repository`}
                  className="font-mono"
                  placeholder="https://github.com/owner/repository"
                  value={repository}
                  aria-invalid={touched && repositoryError !== null}
                  onChange={(event) => setRepository(event.target.value)}
                />
                {touched && repositoryError && (
                  <p role="alert" className="text-xs text-destructive">{repositoryError}</p>
                )}
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={`${prefix}-reference`}>{t('customNodes.install.reference')}</Label>
                <Input
                  id={`${prefix}-reference`}
                  className="font-mono"
                  placeholder={t('customNodes.install.defaultBranch')}
                  value={reference}
                  onChange={(event) => setReference(event.target.value)}
                />
              </div>
            </>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>{t('common.cancel')}</Button>
            <Button type="submit">{t('customNodes.install.prepare')}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

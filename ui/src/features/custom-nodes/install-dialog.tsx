import { useId, useState } from 'react'

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
  const prefix = useId()
  const node = target.kind === 'registry' ? target.node : null
  const [source, setSource] = useState<'registry' | 'git'>(target.kind)
  const [version, setVersion] = useState('')
  const [repository, setRepository] = useState(node?.repository ?? '')
  const [reference, setReference] = useState('')
  const [touched, setTouched] = useState(false)

  const repositoryError = source === 'git' && !HTTPS_URL.test(repository.trim())
    ? 'Enter an https:// repository URL.'
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
          <DialogTitle>{node ? `Install ${node.name}` : 'Install from a Git repository'}</DialogTitle>
          <DialogDescription>
            This prepares an exact plan for review. Nothing is installed or executed until you confirm it.
          </DialogDescription>
        </DialogHeader>
        {node && (node.flagged || node.deprecated) && (
          <Alert>
            <AlertDescription>
              The registry flags this node as {[node.flagged && 'flagged', node.deprecated && 'deprecated'].filter(Boolean).join(' and ')}.
              The review lists every warning.
            </AlertDescription>
          </Alert>
        )}
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => { event.preventDefault(); submit() }}
        >
          {node && node.repository !== null && (
            <RadioGroup
              aria-label="Install source"
              value={source}
              onValueChange={(value) => setSource(value === 'git' ? 'git' : 'registry')}
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id={`${prefix}-registry`} value="registry" />
                <Label htmlFor={`${prefix}-registry`}>Registry release</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id={`${prefix}-git`} value="git" />
                <Label htmlFor={`${prefix}-git`}>Git repository</Label>
              </div>
            </RadioGroup>
          )}
          {source === 'registry' && node ? (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={`${prefix}-version`}>Version (optional)</Label>
              <Input
                id={`${prefix}-version`}
                className="font-mono"
                placeholder={node.latestVersion ?? 'Latest release'}
                value={version}
                onChange={(event) => setVersion(event.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                Leave empty for the latest release. It is resolved once and shown in the review.
              </p>
            </div>
          ) : (
            <>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={`${prefix}-repository`}>Repository URL</Label>
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
                <Label htmlFor={`${prefix}-reference`}>Branch, tag or commit (optional)</Label>
                <Input
                  id={`${prefix}-reference`}
                  className="font-mono"
                  placeholder="Default branch"
                  value={reference}
                  onChange={(event) => setReference(event.target.value)}
                />
              </div>
            </>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
            <Button type="submit">Prepare review</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

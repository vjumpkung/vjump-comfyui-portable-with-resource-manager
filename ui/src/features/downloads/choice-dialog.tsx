import { Button } from '@/shared/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shared/ui/dialog'

export interface Choice<T extends string> {
  id: T
  label: string
  variant?: 'default' | 'outline' | 'destructive'
  disabled?: boolean
}

interface ChoiceDialogProps<T extends string> {
  title: string
  description: string
  choices: readonly Choice<T>[]
  busy: boolean
  onChoose: (choice: T) => void
  /** Escape or the close button; never counts as a destructive choice. */
  onDismiss: () => void
  note?: string
}

/** A modal that asks the user to pick one of a few explicit outcomes. */
export function ChoiceDialog<T extends string>({
  title,
  description,
  choices,
  busy,
  onChoose,
  onDismiss,
  note,
}: ChoiceDialogProps<T>) {
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onDismiss()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription className="break-words [overflow-wrap:anywhere]">
            {description}
          </DialogDescription>
        </DialogHeader>
        {note ? (
          <p className="text-xs text-muted-foreground">{note}</p>
        ) : null}
        <DialogFooter>
          {choices.map((choice) => (
            <Button
              key={choice.id}
              variant={choice.variant ?? 'outline'}
              disabled={busy || choice.disabled}
              onClick={() => onChoose(choice.id)}
            >
              {choice.label}
            </Button>
          ))}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

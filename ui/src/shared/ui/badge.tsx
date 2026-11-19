import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/shared/lib/utils'
import { Slot } from 'radix-ui'

const badgeVariants = cva(
  'group/badge inline-flex w-fit shrink-0 items-center justify-center gap-1 rounded-full border border-transparent px-2.5 py-0.5 text-xs leading-4 font-semibold whitespace-nowrap has-data-[icon=inline-end]:pr-2 has-data-[icon=inline-start]:pl-2 aria-invalid:border-destructive [&>svg]:pointer-events-none [&>svg]:size-3!',
  {
    variants: {
      variant: {
        default: 'border-fill-edge bg-primary text-primary-foreground [a]:hover:bg-primary/85',
        secondary:
          'border-fill-edge bg-secondary text-secondary-foreground [a]:hover:bg-secondary/85',
        accent: 'border-fill-edge bg-accent text-accent-foreground [a]:hover:bg-accent/85',
        muted: 'border-border bg-muted text-muted-foreground',
        destructive: 'bg-destructive text-destructive-foreground [a]:hover:bg-destructive/85',
        outline: 'border-input bg-transparent text-foreground [a]:hover:bg-muted',
        ghost: 'hover:bg-muted hover:text-muted-foreground',
        link: 'text-foreground underline decoration-pink-ink underline-offset-4 hover:decoration-2',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
)

function Badge({
  className,
  variant = 'default',
  asChild = false,
  ...props
}: React.ComponentProps<'span'> & VariantProps<typeof badgeVariants> & { asChild?: boolean }) {
  const Comp = asChild ? Slot.Root : 'span'

  return (
    <Comp
      data-slot="badge"
      data-variant={variant}
      className={cn(badgeVariants({ variant }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }

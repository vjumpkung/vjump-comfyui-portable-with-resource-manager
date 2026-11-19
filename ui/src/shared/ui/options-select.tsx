import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shared/ui/select'

interface OptionsSelectProps<T extends string> {
  id?: string
  /** An empty string shows the placeholder; there is no empty item to pick. */
  value: T | ''
  options: ReadonlyArray<{ value: T; label: string }>
  onValueChange: (value: T) => void
  placeholder?: string
  disabled?: boolean
  size?: 'default' | 'sm'
  className?: string
  'aria-label'?: string
  'aria-describedby'?: string
}

/** A closed list of labelled string options behind the shared Select. */
export function OptionsSelect<T extends string>({
  id,
  value,
  options,
  onValueChange,
  placeholder,
  disabled,
  size,
  className,
  'aria-label': ariaLabel,
  'aria-describedby': ariaDescribedBy,
}: OptionsSelectProps<T>) {
  return (
    <Select value={value} onValueChange={(next) => onValueChange(next as T)} disabled={disabled}>
      <SelectTrigger
        id={id}
        size={size}
        className={className}
        aria-label={ariaLabel}
        aria-describedby={ariaDescribedBy}
      >
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup className="p-0">
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

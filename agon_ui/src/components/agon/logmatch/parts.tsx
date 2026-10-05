import { initials } from '@/lib/members'
import { X } from 'lucide-react'
import { Avatar } from '../Avatar'
import type { TaggedPlayer } from '../PlayerSideEditor'
import { SIDE_COLOURS } from '@/lib/sideColours'
import { cn } from '@/lib/utils'

/** The white rounded card every create-flow section sits in. */
export const CARD = 'flex flex-col gap-3 rounded-[20px] border bg-card p-3.5'

/** The three-dot progress header: done and current steps filled, later ones
 *  outlined, with a label under each. */
export function Stepper({ labels, current }: { labels: string[]; current: number }) {
  return (
    <div className="flex flex-col gap-1.5">
      <ol className="flex items-center gap-1.5" aria-label="Progress">
        {labels.map((label, i) => (
          <li key={label} className={cn('flex items-center gap-1.5', i > 0 && 'flex-1')}>
            {i > 0 && (
              <span
                aria-hidden
                className={cn('h-0.5 flex-1', i <= current ? 'bg-foreground' : 'bg-border')}
              />
            )}
            <span
              aria-current={i === current ? 'step' : undefined}
              aria-label={`Step ${i + 1}: ${label}`}
              className={cn(
                'flex size-[26px] shrink-0 items-center justify-center rounded-full text-xs font-bold',
                i <= current
                  ? 'bg-foreground text-background'
                  : 'border bg-card text-muted-foreground',
              )}
            >
              {i + 1}
            </span>
          </li>
        ))}
      </ol>
      <div className="flex justify-between text-[11px] font-bold text-muted-foreground">
        {labels.map((label, i) => (
          <span key={label} className={cn(i === current && 'text-foreground')}>
            {label}
          </span>
        ))}
      </div>
    </div>
  )
}

/** A segmented single-choice control (radio group styled as tabs). */
export function Segmented<T extends string>({
  label,
  options,
  value,
  onChange,
  className,
}: {
  label: string
  options: { value: T; label: string }[]
  value: T
  onChange: (value: T) => void
  className?: string
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn('grid gap-1 rounded-xl bg-muted p-1', className)}
      style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            'h-10 rounded-[9px] px-2 text-sm transition-colors',
            value === o.value
              ? 'bg-card font-bold text-foreground shadow-sm'
              : 'font-semibold text-muted-foreground hover:text-foreground',
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

/** A titled block of the form, e.g. "Sport" or "Your side". */
export function FormSection({
  title,
  aside,
  htmlFor,
  children,
}: {
  title: string
  aside?: React.ReactNode
  htmlFor?: string
  children: React.ReactNode
}) {
  const Heading = htmlFor ? 'label' : 'h2'
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline justify-between px-1">
        <Heading htmlFor={htmlFor} className="font-display text-[17px] font-bold">
          {title}
        </Heading>
        {aside && <span className="text-xs font-semibold text-muted-foreground">{aside}</span>}
      </div>
      {children}
    </section>
  )
}

/** A tagged player as a removable pill, tagged "Invited" (an Agon user who
 *  isn't you) or "Guest" (not on Agon). */
export function PlayerChip({
  player,
  currentUserId,
  onRemove,
}: {
  player: TaggedPlayer
  currentUserId?: string
  onRemove: () => void
}) {
  const isYou = player.kind === 'user' && player.id === currentUserId
  return (
    <span className="inline-flex h-9 items-center gap-2 rounded-full bg-secondary pl-1 pr-2 text-[13px] font-semibold">
      {player.kind === 'user' ? (
        <Avatar name={player.name} imageUrl={player.imageUrl} size="md" />
      ) : (
        <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-[10px] font-bold text-muted-foreground">
          {initials(player.name)}
        </span>
      )}
      <span className="max-w-36 truncate" data-testid="tagged-player-name" data-player-name={player.name}>
        {isYou ? 'You' : player.name}
      </span>
      {!isYou &&
        (player.kind === 'user' ? (
          <span className="text-[11px] font-semibold text-amber-700 dark:text-amber-300">Invited</span>
        ) : (
          <span className="text-[11px] font-semibold text-muted-foreground">Guest</span>
        ))}
      <button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${player.name}`}
        className="flex size-6 items-center justify-center rounded-full text-muted-foreground hover:text-foreground"
      >
        <X className="size-3.5" />
      </button>
    </span>
  )
}

/** The side-colour swatches, as a radio group. */
export function ColourSwatches({
  label,
  value,
  onChange,
}: {
  label: string
  value: string
  onChange: (hex: string) => void
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex flex-wrap gap-2.5">
      {SIDE_COLOURS.map((c) => (
        <button
          key={c.hex}
          type="button"
          role="radio"
          aria-checked={value === c.hex}
          aria-label={c.label}
          onClick={() => onChange(c.hex)}
          className={cn(
            'size-8 rounded-full border border-black/10 transition-shadow',
            value === c.hex && 'ring-2 ring-foreground ring-offset-2 ring-offset-card',
          )}
          style={{ backgroundColor: c.hex }}
        />
      ))}
    </div>
  )
}

/** A labelled on/off row with an optional second line. */
export function ToggleRow({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string
  hint?: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <label className="flex min-h-11 items-center justify-between gap-3 text-sm font-semibold">
      <span>
        {label}
        {hint && <span className="block text-xs font-normal text-muted-foreground">{hint}</span>}
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="size-[22px] shrink-0 accent-primary"
      />
    </label>
  )
}

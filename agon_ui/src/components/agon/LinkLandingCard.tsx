import { cn } from '@/lib/utils'

export interface LinkLandingCardProps extends React.HTMLAttributes<HTMLDivElement> {
  /** The blue-tinted icon badge at the top of the card. Omitted for states
   *  that show plain text only (loading, "not found", etc). */
  icon?: React.ReactNode
  heading?: React.ReactNode
  description?: React.ReactNode
}

/**
 * The centered white card that every "reached from a link" landing screen
 * shares — accepting an invite, joining a match via a join-link, pairing a
 * Garmin watch. Renders the icon badge/heading/description chrome common to
 * all three, then any state-specific content (buttons, a form, a roster
 * preview, …) as `children`. See the "Agon redesign" canvas's link-landing
 * reference sheet (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 */
export function LinkLandingCard({
  icon,
  heading,
  description,
  children,
  className,
  ...props
}: LinkLandingCardProps) {
  return (
    <div
      className={cn(
        'mx-auto flex max-w-md flex-col items-center rounded-2xl border bg-card p-8 text-center',
        className,
      )}
      {...props}
    >
      {icon && (
        <div className="mb-4 flex size-14 items-center justify-center rounded-full bg-accent text-accent-foreground">
          {icon}
        </div>
      )}
      {heading && (
        <h2 className="mb-1 font-display text-xl font-extrabold text-foreground">{heading}</h2>
      )}
      {description && <div className="mb-6 text-sm text-muted-foreground">{description}</div>}
      {children}
    </div>
  )
}

import { MapPin, Navigation } from 'lucide-react'
import type { components } from '@/types/api'
import { directionsUrl } from '@/lib/location'
import { cn } from '@/lib/utils'

type Location = components['schemas']['Location']

/** The green map-pin tile marking a location picked from Google Places. */
export function PlaceChip({ size = 'sm' }: { size?: 'sm' | 'lg' }) {
  return (
    <span
      className={cn(
        'flex shrink-0 items-center justify-center bg-gradient-to-br from-emerald-100 to-emerald-200/70 dark:from-emerald-900/40 dark:to-emerald-800/30',
        size === 'lg' ? 'size-9 rounded-xl' : 'size-6 rounded-lg',
      )}
    >
      <MapPin
        className={cn('text-emerald-900 dark:text-emerald-200', size === 'lg' ? 'size-[18px]' : 'size-3.5')}
        fill="currentColor"
        stroke="white"
        strokeWidth={1.5}
      />
    </span>
  )
}

export function DirectionsButton({ location }: { location: Location }) {
  const href = directionsUrl(location)
  if (!href) return null
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="ml-auto flex h-8 shrink-0 items-center gap-1.5 rounded-full border border-edge bg-card px-3 text-[13px] font-bold text-foreground hover:bg-muted"
    >
      <Navigation className="size-3.5" />
      Directions
    </a>
  )
}

/**
 * A match's location under its title. A place picked from Google Places
 * gets the green pin chip and, unless `showDirections` is off, a Directions
 * button; a free-typed location is plain text with an outline pin, since
 * there's nothing reliable to route to.
 */
export function MatchLocation({
  location,
  size = 'sm',
  showDirections = true,
  className,
}: {
  location: Location
  size?: 'sm' | 'lg'
  showDirections?: boolean
  className?: string
}) {
  const linked = directionsUrl(location) !== undefined
  return (
    <div className={cn('flex min-w-0 items-center', size === 'lg' ? 'gap-3 text-base' : 'gap-2 text-sm', className)}>
      {linked ? (
        <PlaceChip size={size} />
      ) : (
        <span className={cn('flex shrink-0 items-center justify-center', size === 'lg' ? 'size-9' : 'size-6')}>
          <MapPin className={cn('text-muted-foreground', size === 'lg' ? 'size-5' : 'size-4')} />
        </span>
      )}
      <span className={cn('truncate', linked ? 'font-semibold text-foreground' : 'text-muted-foreground')}>
        {location.text}
      </span>
      {linked && showDirections && <DirectionsButton location={location} />}
    </div>
  )
}

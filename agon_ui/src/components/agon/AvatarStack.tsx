import { cn } from '@/lib/utils'
import { Avatar, type AvatarProps } from './Avatar'

export interface AvatarStackProps extends React.HTMLAttributes<HTMLDivElement> {
  people: { name: string; imageUrl?: string }[]
  max?: number
  size?: AvatarProps['size']
}

/**
 * Overlapping avatar cluster (attendee previews, "you played with" rows) —
 * a `ring-2 ring-card` outline on each avatar so they read as stacked
 * against any card background.
 */
export function AvatarStack({
  people,
  max = 3,
  size = 'sm',
  className,
  ...props
}: AvatarStackProps) {
  return (
    <div className={cn('flex -space-x-2', className)} {...props}>
      {people.slice(0, max).map((p, i) => (
        <Avatar
          key={i}
          name={p.name}
          imageUrl={p.imageUrl}
          size={size}
          className="ring-2 ring-card"
        />
      ))}
    </div>
  )
}

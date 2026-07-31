import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'
import { initials as toInitials } from '@/lib/members'

const avatarVariants = cva(
  'inline-flex shrink-0 items-center justify-center rounded-full bg-accent font-medium text-accent-foreground select-none',
  {
    variants: {
      size: {
        sm: 'size-5 text-[8px]',
        md: 'size-7 text-[10px]',
        lg: 'size-9 text-xs',
        xl: 'size-16 text-xl',
      },
      ring: {
        none: '',
        winner: 'ring-2 ring-primary',
        you: 'ring-2 ring-primary',
      },
    },
    defaultVariants: {
      size: 'md',
      ring: 'none',
    },
  },
)

export interface AvatarProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof avatarVariants> {
  /** Display name; initials are derived from it when `imageUrl` is absent. */
  name: string
  /** Optional profile image; falls back to initials when not provided. */
  imageUrl?: string
}

/**
 * Circular avatar showing a profile image, or the person's initials on a tinted
 * background. Sizes and an optional accent ring (e.g. to mark a match winner or
 * "you") match the Agon feed/detail mockups.
 */
export function Avatar({
  name,
  imageUrl,
  size,
  ring,
  className,
  ...props
}: AvatarProps) {
  return (
    <span
      className={cn(avatarVariants({ size, ring }), 'overflow-hidden', className)}
      title={name}
      {...props}
    >
      {imageUrl ? (
        <img
          src={imageUrl}
          alt={name}
          className="size-full object-cover"
          loading="lazy"
        />
      ) : (
        toInitials(name)
      )}
    </span>
  )
}

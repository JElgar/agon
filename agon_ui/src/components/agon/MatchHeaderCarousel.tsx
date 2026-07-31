import { useEffect, useState } from 'react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import {
  Carousel,
  CarouselContent,
  CarouselItem,
  CarouselPrevious,
  CarouselNext,
  type CarouselApi,
} from '@/components/ui/carousel'

type Photo = components['schemas']['Photo']

export interface MatchHeaderCarouselProps {
  photos: Photo[]
  className?: string
}

/**
 * Match header images. A single photo renders as a plain banner; multiple photos
 * become a swipeable carousel with position dots and (on pointer/large screens)
 * prev/next buttons. Swipe works on every screen size — the buttons are just an
 * affordance for mouse users and are hidden on small screens.
 */
export function MatchHeaderCarousel({ photos, className }: MatchHeaderCarouselProps) {
  const [api, setApi] = useState<CarouselApi>()
  const [selected, setSelected] = useState(0)

  useEffect(() => {
    if (!api) return
    const onSelect = () => setSelected(api.selectedScrollSnap())
    onSelect()
    api.on('select', onSelect)
    return () => {
      api.off('select', onSelect)
    }
  }, [api])

  if (photos.length === 0) return null

  // Single image: no carousel chrome needed.
  if (photos.length === 1) {
    return (
      <img
        src={photos[0].image_url}
        alt=""
        className={cn('h-40 w-full rounded-xl border object-cover', className)}
        loading="lazy"
      />
    )
  }

  return (
    <Carousel setApi={setApi} className={cn('w-full', className)} opts={{ loop: true }}>
      <CarouselContent>
        {photos.map((photo, i) => (
          <CarouselItem key={i}>
            <img
              src={photo.image_url}
              alt=""
              className="h-40 w-full rounded-xl border object-cover"
              loading="lazy"
            />
          </CarouselItem>
        ))}
      </CarouselContent>

      {/* Mouse affordance; hidden on small (touch) screens where swipe is natural. */}
      <CarouselPrevious className="left-2 hidden sm:flex" />
      <CarouselNext className="right-2 hidden sm:flex" />

      {/* Position dots. */}
      <div className="pointer-events-none absolute inset-x-0 bottom-2 flex justify-center gap-1.5">
        {photos.map((_, i) => (
          <button
            key={i}
            type="button"
            aria-label={`Go to image ${i + 1}`}
            onClick={() => api?.scrollTo(i)}
            className={cn(
              'pointer-events-auto size-1.5 rounded-full bg-white/60 transition-all',
              i === selected && 'w-4 bg-white',
            )}
          />
        ))}
      </div>
    </Carousel>
  )
}

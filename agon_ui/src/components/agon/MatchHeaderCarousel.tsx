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
  /** Full-bleed hero treatment (taller, no border/radius) for the football
   *  match-detail page's photo header (`Match.dc.html`), instead of the
   *  usual bordered rounded banner. */
  hero?: boolean
}

/**
 * Match header images. A single photo renders as a plain banner; multiple photos
 * become a swipeable carousel with position dots and (on pointer/large screens)
 * prev/next buttons. Swipe works on every screen size — the buttons are just an
 * affordance for mouse users and are hidden on small screens.
 */
export function MatchHeaderCarousel({ photos, className, hero }: MatchHeaderCarouselProps) {
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

  const imageClassName = hero
    ? 'h-[250px] w-full rounded-none border-0 object-cover'
    : 'h-40 w-full rounded-xl border object-cover'

  // Single image: no carousel chrome needed.
  if (photos.length === 1) {
    return (
      <img
        src={photos[0].image_url}
        alt=""
        className={cn(imageClassName, className)}
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
              className={imageClassName}
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

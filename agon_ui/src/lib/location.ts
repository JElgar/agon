import type { components } from '@/types/api'

type Location = components['schemas']['Location']

/**
 * A Google Maps directions link for a match location, or `undefined` when
 * there isn't enough to build a reliable one. Gated on real coordinates
 * (only ever present once a Places suggestion was picked) rather than on
 * `location` merely existing — a maps query built from free text alone can
 * point at the wrong place entirely, so plain-text-only locations show no
 * directions link at all (see `Location`'s API doc comment).
 */
export function directionsUrl(location: Location | null | undefined): string | undefined {
  if (!location || location.latitude == null || location.longitude == null) return undefined
  const params = new URLSearchParams({
    api: '1',
    destination: `${location.latitude},${location.longitude}`,
  })
  if (location.place_id) params.set('destination_place_id', location.place_id)
  return `https://www.google.com/maps/dir/?${params.toString()}`
}

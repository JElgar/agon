import { useEffect, useRef, useState } from 'react'
import { Input } from '@/components/ui/input'
import {
  isGoogleMapsConfigured,
  loadGoogleMapsPlaces,
  type GoogleAutocomplete,
} from '@/lib/googleMaps'

export interface LocationValue {
  text: string
  latitude?: number
  longitude?: number
  place_id?: string
}

/**
 * Free-text match location, upgraded to a real place (coordinates + Google
 * Place ID) when Places Autocomplete is configured (`VITE_GOOGLE_MAPS_API_KEY`
 * — see `googleMaps.ts`) and the user picks a suggestion. Typing without
 * picking one still saves fine as plain text; it just won't get a "get
 * directions" link on the match page (see `MatchDetailPage`'s gating on
 * `latitude`/`longitude`).
 *
 * Editing the text after a place was picked falls back to plain text again
 * (clears the coordinates/place id) — the enriched values only ever come
 * from the Autocomplete widget's own `place_changed` event, never from
 * guessing at freehand edits.
 */
export function LocationField({
  id,
  value,
  onChange,
  placeholder = 'e.g. Pitch 5, Leather Lane',
  className,
}: {
  id?: string
  value: LocationValue | null
  onChange: (value: LocationValue | null) => void
  placeholder?: string
  className?: string
}) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [text, setText] = useState(value?.text ?? '')

  // Re-seed local text when the *record underneath* changes (e.g. this field
  // gets reused for a different match) — not on every parent re-render,
  // which would fight the user mid-keystroke.
  useEffect(() => {
    setText(value?.text ?? '')
  }, [value?.text])

  useEffect(() => {
    if (!isGoogleMapsConfigured() || !inputRef.current) return
    let cancelled = false
    let autocomplete: GoogleAutocomplete | undefined

    loadGoogleMapsPlaces()
      .then(() => {
        if (cancelled || !inputRef.current || !window.google) return
        autocomplete = new window.google.maps.places.Autocomplete(inputRef.current, {
          fields: ['place_id', 'formatted_address', 'geometry'],
        })
        autocomplete.addListener('place_changed', () => {
          const place = autocomplete!.getPlace()
          const resolvedText = place.formatted_address ?? inputRef.current?.value ?? ''
          setText(resolvedText)
          onChange({
            text: resolvedText,
            latitude: place.geometry?.location?.lat(),
            longitude: place.geometry?.location?.lng(),
            place_id: place.place_id,
          })
        })
      })
      .catch(() => {
        // No autocomplete this session (network hiccup, ad blocker, bad
        // key) — the field still works as plain text, just without
        // suggestions or coordinates.
      })

    return () => {
      cancelled = true
    }
    // Wired up once per mount — `onChange` is a fresh closure each render,
    // and re-running this to rebind it would just recreate the same widget.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <Input
      id={id}
      ref={inputRef}
      value={text}
      placeholder={placeholder}
      className={className}
      onChange={(e) => {
        const next = e.target.value
        setText(next)
        onChange(next.trim() ? { text: next.trim() } : null)
      }}
    />
  )
}

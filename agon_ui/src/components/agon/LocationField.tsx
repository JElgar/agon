import { useEffect, useRef, useState } from 'react'
import {
  Combobox,
  ComboboxContent,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox'
import {
  isGoogleMapsConfigured,
  loadGoogleMapsPlaces,
  type GoogleAutocompleteSessionToken,
  type GooglePlacePrediction,
} from '@/lib/googleMaps'

export interface LocationValue {
  text: string
  latitude?: number
  longitude?: number
  place_id?: string
}

/** How long to wait after typing stops before hitting the Places API. */
const SEARCH_DEBOUNCE_MS = 250
/** Google requires the query token to apply this way, not case-sensitively. */
const MIN_QUERY_LENGTH = 2

/**
 * Free-text match location, upgraded to a real place (coordinates + Google
 * Place ID) when Places Autocomplete is configured (`VITE_GOOGLE_MAPS_API_KEY`
 * — see `googleMaps.ts`) and the user picks a suggestion. Typing without
 * picking one still saves fine as plain text; it just won't get a "get
 * directions" link on the match page (see `MatchDetailPage`'s gating on
 * `latitude`/`longitude`).
 *
 * Built on `AutocompleteSuggestion.fetchAutocompleteSuggestions` (the "Places
 * API (New)" data-only call), not the older `google.maps.places.Autocomplete`
 * widget — that widget, and its `AutocompleteService` sibling, are blocked
 * for any Google Cloud project that hadn't already used the Places API
 * before March 1st, 2025 (ours hadn't — the key was only provisioned in PR
 * #154, September 2026), so they just throw up Google's generic "This page
 * can't load Google Maps correctly" overlay. Suggestions are rendered in our
 * own `Combobox`, not Google's widget, so this field keeps its usual styling.
 *
 * Editing the text after a place was picked falls back to plain text again
 * (clears the coordinates/place id) — the enriched values only ever come
 * from picking a suggestion, never from guessing at freehand edits.
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
  const [text, setText] = useState(value?.text ?? '')
  const [debounced, setDebounced] = useState('')
  const [predictions, setPredictions] = useState<GooglePlacePrediction[]>([])
  const [mapsReady, setMapsReady] = useState(false)
  const sessionTokenRef = useRef<GoogleAutocompleteSessionToken | undefined>(undefined)

  // Re-seed local text when the *record underneath* changes (e.g. this field
  // gets reused for a different match) — not on every parent re-render,
  // which would fight the user mid-keystroke.
  useEffect(() => {
    setText(value?.text ?? '')
  }, [value?.text])

  useEffect(() => {
    if (!isGoogleMapsConfigured()) return
    let cancelled = false
    loadGoogleMapsPlaces()
      .then(() => {
        if (!cancelled) setMapsReady(true)
      })
      .catch(() => {
        // No autocomplete this session (network hiccup, ad blocker, bad
        // key) — the field still works as plain text, just without
        // suggestions or coordinates.
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    const t = setTimeout(() => setDebounced(text.trim()), SEARCH_DEBOUNCE_MS)
    return () => clearTimeout(t)
  }, [text])

  useEffect(() => {
    if (!mapsReady || debounced.length < MIN_QUERY_LENGTH || !window.google) {
      setPredictions([])
      return
    }
    if (!sessionTokenRef.current) {
      sessionTokenRef.current = new window.google.maps.places.AutocompleteSessionToken()
    }
    let cancelled = false
    window.google.maps.places.AutocompleteSuggestion.fetchAutocompleteSuggestions({
      input: debounced,
      sessionToken: sessionTokenRef.current,
    })
      .then(({ suggestions }) => {
        if (cancelled) return
        setPredictions(
          suggestions
            .map((s) => s.placePrediction)
            .filter((p): p is GooglePlacePrediction => !!p),
        )
      })
      .catch(() => {
        if (!cancelled) setPredictions([])
      })
    return () => {
      cancelled = true
    }
  }, [mapsReady, debounced])

  const pickPrediction = async (prediction: GooglePlacePrediction) => {
    const place = prediction.toPlace()
    await place.fetchFields({ fields: ['formattedAddress', 'location'] })
    const resolvedText = place.formattedAddress ?? prediction.text.toString()
    setText(resolvedText)
    setPredictions([])
    // Picking a place ends this search session — the next one gets a fresh
    // token, per Google's session-token billing model.
    sessionTokenRef.current = undefined
    onChange({
      text: resolvedText,
      latitude: place.location?.lat(),
      longitude: place.location?.lng(),
      place_id: prediction.placeId,
    })
  }

  return (
    <Combobox
      items={predictions}
      filter={null}
      inputValue={text}
      onInputValueChange={(next) => {
        setText(next)
        onChange(next.trim() ? { text: next.trim() } : null)
      }}
      // Keep the input showing exactly what the user typed/picked, never a
      // stringified prediction object.
      itemToStringLabel={() => ''}
      onValueChange={(next) => {
        const prediction = next as GooglePlacePrediction | null
        if (prediction) void pickPrediction(prediction)
      }}
    >
      <ComboboxInput
        id={id}
        placeholder={placeholder}
        showTrigger={false}
        className={className}
      />
      <ComboboxContent>
        <ComboboxList>
          {(prediction: GooglePlacePrediction) => (
            <ComboboxItem key={prediction.placeId} value={prediction}>
              <span className="flex-1 truncate">{prediction.text.toString()}</span>
            </ComboboxItem>
          )}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}

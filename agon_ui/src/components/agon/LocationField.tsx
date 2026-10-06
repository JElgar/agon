import { useEffect, useRef, useState } from 'react'
import { MapPin, Search, X } from 'lucide-react'
import {
  Combobox,
  ComboboxContent,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox'
import { InputGroupAddon } from '@/components/ui/input-group'
import { cn } from '@/lib/utils'
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

/** The place last picked from suggestions — kept separately from `text` so a
 *  rename afterwards doesn't lose the coordinates/place id it came with. */
interface LinkedPlace {
  address: string
  latitude?: number
  longitude?: number
  place_id: string
}

/**
 * One field for a match's location, with two states:
 *
 * - **Unlinked** — a plain search box. Typing always sets `text`; live
 *   suggestions (Places API) appear underneath, and picking one links the
 *   place (saves coordinates + Google Place ID) and switches to the linked
 *   card below.
 * - **Linked** — a map-thumbnail card. Its name is a real `<input>`, still
 *   freely editable (e.g. to rename "Mint Street Pitches" to "Pitch 2") —
 *   editing it only renames the card, never re-triggers suggestions and
 *   never touches the saved coordinates/place id. The saved address shows
 *   underneath whenever it differs from the current name. The cross unlinks
 *   the place (clearing coordinates/place id) and returns to the unlinked
 *   search box.
 *
 * Suggestions come from `AutocompleteSuggestion.fetchAutocompleteSuggestions`
 * (see `googleMaps.ts`) and render in our own `Combobox`, so this field
 * keeps its usual styling rather than Google's own widget.
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
  const [linkedPlace, setLinkedPlace] = useState<LinkedPlace | null>(
    value?.place_id
      ? {
          address: value.text,
          latitude: value.latitude,
          longitude: value.longitude,
          place_id: value.place_id,
        }
      : null,
  )
  const [debounced, setDebounced] = useState('')
  const [predictions, setPredictions] = useState<GooglePlacePrediction[]>([])
  const [mapsReady, setMapsReady] = useState(false)
  const sessionTokenRef = useRef<GoogleAutocompleteSessionToken | undefined>(undefined)

  // Re-seed from the *record underneath* when it changes (e.g. this field
  // gets reused for a different match) — not on every parent re-render,
  // which would fight the user mid-keystroke.
  useEffect(() => {
    setText(value?.text ?? '')
    setLinkedPlace(
      value?.place_id
        ? {
            address: value.text,
            latitude: value.latitude,
            longitude: value.longitude,
            place_id: value.place_id,
          }
        : null,
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value?.place_id])

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
    // Linked: typing only renames the card, it's never a fresh search.
    if (linkedPlace || !mapsReady || debounced.length < MIN_QUERY_LENGTH || !window.google) {
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
  }, [linkedPlace, mapsReady, debounced])

  const pickPrediction = async (prediction: GooglePlacePrediction) => {
    const place = prediction.toPlace()
    await place.fetchFields({ fields: ['formattedAddress', 'location'] })
    const resolvedText = place.formattedAddress ?? prediction.text.toString()
    setText(resolvedText)
    setPredictions([])
    // Picking a place ends this search session — the next one gets a fresh
    // token, per Google's session-token billing model.
    sessionTokenRef.current = undefined
    const latitude = place.location?.lat()
    const longitude = place.location?.lng()
    setLinkedPlace({ address: resolvedText, latitude, longitude, place_id: prediction.placeId })
    onChange({ text: resolvedText, latitude, longitude, place_id: prediction.placeId })
  }

  const unlink = () => {
    setLinkedPlace(null)
    setText('')
    onChange(null)
  }

  if (linkedPlace) {
    return (
      <div
        className={cn(
          'flex items-center gap-3 rounded-2xl border border-input bg-card p-3.5 shadow-xs',
          className,
        )}
      >
        <span className="relative flex size-12 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-gradient-to-br from-emerald-100 to-emerald-200/70 dark:from-emerald-900/40 dark:to-emerald-800/30">
          <MapPin className="size-5 text-primary" fill="currentColor" stroke="white" strokeWidth={1.5} />
        </span>
        <div className="min-w-0 flex-1">
          <input
            id={id}
            value={text}
            onChange={(e) => {
              const next = e.target.value
              setText(next)
              onChange({
                text: next,
                latitude: linkedPlace.latitude,
                longitude: linkedPlace.longitude,
                place_id: linkedPlace.place_id,
              })
            }}
            className="w-full min-w-0 border-none bg-transparent p-0 text-sm font-semibold text-foreground outline-none"
          />
          {linkedPlace.address !== text && (
            <p className="truncate text-xs text-muted-foreground">{linkedPlace.address}</p>
          )}
        </div>
        <button
          type="button"
          aria-label="Remove location"
          onClick={unlink}
          className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground hover:bg-muted/80"
        >
          <X className="size-3.5" />
        </button>
      </div>
    )
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
      <ComboboxInput id={id} placeholder={placeholder} showTrigger={false} className={className}>
        <InputGroupAddon align="inline-start" className="text-muted-foreground">
          <Search className="size-4" />
        </InputGroupAddon>
      </ComboboxInput>
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

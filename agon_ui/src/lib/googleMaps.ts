import { getRuntimeEnv } from '@/utils/runtime-env'

const env = getRuntimeEnv()

/**
 * Google Maps JS API key, used only for Places Autocomplete on the match
 * location field (see `LocationField`). Public/client-safe — a Maps
 * JavaScript API key is meant to ship in the browser bundle and is secured
 * by restricting it (HTTP referrer) in the Google Cloud Console, not by
 * keeping it secret. Leave unset to develop with the location field as
 * plain free text only — no autocomplete, no coordinates, no directions
 * link — same "absent config means the feature just stays off" convention
 * as `isFirebaseConfigured`.
 */
const googleMapsApiKey = env.VITE_GOOGLE_MAPS_API_KEY

/** True if a value is present and isn't an un-substituted `${VAR}` template. */
function isSet(value: string | undefined): boolean {
  return !!value && !value.startsWith('${')
}

export function isGoogleMapsConfigured(): boolean {
  return isSet(googleMapsApiKey)
}

declare global {
  interface Window {
    google?: {
      maps: {
        places: {
          AutocompleteSuggestion: {
            fetchAutocompleteSuggestions(request: {
              input: string
              sessionToken?: GoogleAutocompleteSessionToken
            }): Promise<{ suggestions: GoogleAutocompleteSuggestion[] }>
          }
          AutocompleteSessionToken: new () => GoogleAutocompleteSessionToken
        }
      }
    }
    __agonGoogleMapsLoaded?: () => void
  }
}

/** Opaque — just threaded through to group one search's keystrokes (and the
 *  place it ends in) into a single Places session for billing purposes. */
export type GoogleAutocompleteSessionToken = object

export interface GooglePlace {
  formattedAddress?: string
  location?: { lat(): number; lng(): number }
  fetchFields(opts: { fields: string[] }): Promise<{ place: GooglePlace }>
}

export interface GooglePlacePrediction {
  placeId: string
  text: { toString(): string }
  toPlace(): GooglePlace
}

export interface GoogleAutocompleteSuggestion {
  placePrediction: GooglePlacePrediction | null
}

let loadPromise: Promise<void> | undefined

/**
 * Loads the Google Maps JS API's `places` library, once, idempotently — a
 * second `LocationField` on the same page reuses the same load. Resolves
 * immediately if the script is already present (e.g. a fast remount).
 */
export function loadGoogleMapsPlaces(): Promise<void> {
  if (window.google?.maps?.places) return Promise.resolve()
  if (loadPromise) return loadPromise

  loadPromise = new Promise((resolve, reject) => {
    window.__agonGoogleMapsLoaded = () => resolve()
    const script = document.createElement('script')
    script.src = `https://maps.googleapis.com/maps/api/js?key=${encodeURIComponent(googleMapsApiKey ?? '')}&libraries=places&loading=async&callback=__agonGoogleMapsLoaded`
    script.async = true
    script.onerror = () => reject(new Error('Failed to load the Google Maps JS API'))
    document.head.appendChild(script)
  })
  return loadPromise
}

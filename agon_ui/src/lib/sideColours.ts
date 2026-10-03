/** A pickable colour for an ad-hoc (no-team) match side — the `colour` field
 *  the create-match API now requires whenever a side has no `team_id` (a
 *  linked team is the colour's source of truth instead, once teams have
 *  one). `label` is the plural kit-style name ("Blues", "Whites") used to
 *  auto-fill the side's name when a colour is picked before any name is
 *  typed, and `keywords` are what's matched (case-insensitively) in a typed
 *  name to auto-pick this colour the other way round. */
export interface SideColourOption {
  hex: string
  label: string
  keywords: string[]
}

export const SIDE_COLOURS: SideColourOption[] = [
  { hex: '#2952D9', label: 'Blues', keywords: ['blue', 'navy'] },
  { hex: '#DC2626', label: 'Reds', keywords: ['red'] },
  { hex: '#16A34A', label: 'Greens', keywords: ['green'] },
  { hex: '#EAB308', label: 'Yellows', keywords: ['yellow', 'gold'] },
  { hex: '#F97316', label: 'Oranges', keywords: ['orange'] },
  { hex: '#9333EA', label: 'Purples', keywords: ['purple', 'violet'] },
  { hex: '#EC4899', label: 'Pinks', keywords: ['pink'] },
  { hex: '#0F172A', label: 'Blacks', keywords: ['black'] },
  { hex: '#E2E8F0', label: 'Whites', keywords: ['white'] },
  { hex: '#64748B', label: 'Greys', keywords: ['grey', 'gray'] },
]

/** The first colour whose keyword appears in `name` (case-insensitive), if any. */
export function colourFromName(name: string): string | undefined {
  const lower = name.toLowerCase()
  return SIDE_COLOURS.find((c) => c.keywords.some((k) => lower.includes(k)))?.hex
}

/** The kit-style name for a colour (e.g. "Blues"), for auto-filling an
 *  untouched side name once a colour is picked. */
export function nameFromColour(hex: string): string | undefined {
  return SIDE_COLOURS.find((c) => c.hex === hex)?.label
}

/** A sensible default colour for a freshly created ad-hoc side, so the
 *  create-match API's now-required `colour` is satisfied even before the
 *  user has touched the picker. `other` (the sibling side's current colour,
 *  if it's also colour-picked) is avoided so the two sides don't default to
 *  the same colour. */
export function defaultSideColour(other?: string): string {
  return SIDE_COLOURS.find((c) => c.hex !== other)?.hex ?? SIDE_COLOURS[0].hex
}

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

/** A small fixed palette to derive a team's crest color from, since the
 *  schema has no team-colour field at all. Judgment call: hash `team_id`
 *  (sum of char codes, mod palette length) to a deterministic entry, so the
 *  same team always renders the same colour without needing real brand data. */
const TEAM_CREST_PALETTE = [
  '#123E5B',
  '#1F4D3A',
  '#7A1F3D',
  '#6B2F12',
  '#3B5B12',
  '#4B2E68',
  '#8A4B08',
  '#1E3FA8',
]

export function teamCrestColor(teamId: string): string {
  let sum = 0
  for (let i = 0; i < teamId.length; i++) sum += teamId.charCodeAt(i)
  return TEAM_CREST_PALETTE[sum % TEAM_CREST_PALETTE.length]
}

/** A side's real identity colour: its own stored `colour` first (ad-hoc and
 *  derby sides carry one), else a team-linked side's `teamCrestColor`,
 *  else `undefined` when neither applies. Checking `colour` before
 *  `team_id` matters for a derby side (two sides sharing one `team_id`),
 *  which has both. */
export function resolveSideColour(side: { colour?: string | null; team_id?: string | null } | undefined): string | undefined {
  return side?.colour ?? (side?.team_id ? teamCrestColor(side.team_id) : undefined)
}

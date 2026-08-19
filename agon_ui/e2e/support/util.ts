/** Escapes a string for safe interpolation into a `RegExp` — used when
 *  matching an accessible name that contains generated test data (e.g. a
 *  match's opponent name) that isn't itself a regex. */
export function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

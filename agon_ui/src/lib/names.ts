/**
 * The API only stores a single `name` string (see CLAUDE.md — backend is not
 * being changed). The UI collects first/last name separately for a better
 * signup/edit experience, then joins them into that one field before
 * sending anything to the API, and splits it back apart when displaying
 * an existing name in an editable form.
 */

/** Join first + last name into the single string the API expects. */
export function joinName(firstName: string, lastName: string): string {
  return [firstName.trim(), lastName.trim()].filter(Boolean).join(' ')
}

/**
 * Split a stored full name back into first/last name for editing.
 * Best-effort: everything up to the first space is the first name, the
 * remainder (if any) is the last name.
 */
export function splitName(fullName: string): { firstName: string; lastName: string } {
  const trimmed = fullName.trim()
  if (!trimmed) return { firstName: '', lastName: '' }
  const [firstName, ...rest] = trimmed.split(/\s+/)
  return { firstName, lastName: rest.join(' ') }
}

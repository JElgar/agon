/** A required env var for the e2e suite — throws with a pointer to the docs
 *  instead of a bare `undefined` surfacing three calls later. */
export function requireEnv(name: string): string {
  const value = process.env[name]
  if (!value) {
    throw new Error(`${name} must be set to run the e2e suite — see e2e/README.md`)
  }
  return value
}

import posthog from 'posthog-js'
import { getRuntimeEnv } from '@/utils/runtime-env'

const env = getRuntimeEnv()

/** True if a value is present and isn't an un-substituted `${VAR}` template —
 *  same convention as isFirebaseConfigured (see src/lib/firebase.ts). */
function isSet(value: string | undefined): boolean {
  return !!value && !value.startsWith('${')
}

export function isPosthogConfigured(): boolean {
  return isSet(env.VITE_POSTHOG_KEY) && isSet(env.VITE_POSTHOG_HOST)
}

/**
 * Initializes PostHog in cookieless mode: `persistence: 'memory'` keeps
 * distinct_id and session state in memory only (no cookies, no
 * localStorage), so no cookie-consent banner is needed. A reload starts a
 * fresh anonymous identity unless `identify` below has tied the session to
 * a signed-in account.
 */
export function initPosthog(): void {
  if (!isPosthogConfigured()) return

  posthog.init(env.VITE_POSTHOG_KEY, {
    api_host: env.VITE_POSTHOG_HOST,
    persistence: 'memory',
    person_profiles: 'identified_only',
  })
}

/** Ties the current PostHog session to the signed-in account's internal
 *  user id (never the Supabase `sub` — see CLAUDE.md on internal ids). */
export function identifyPosthogUser(userId: string): void {
  if (!isPosthogConfigured()) return
  posthog.identify(userId)
}

/** Clears the identified user on sign-out, so the next session starts a
 *  fresh anonymous identity rather than continuing to attribute events to
 *  the account that just signed out. */
export function resetPosthogUser(): void {
  if (!isPosthogConfigured()) return
  posthog.reset()
}

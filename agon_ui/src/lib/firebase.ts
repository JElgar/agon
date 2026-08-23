import { initializeApp, type FirebaseApp } from 'firebase/app'
import { getRuntimeEnv } from '@/utils/runtime-env'

const env = getRuntimeEnv()

/** Firebase Web SDK config — all public, client-safe identifiers (Firebase
 *  ships these into every web app's bundle by design). Sourced from the
 *  `firebaseWebConfig` / `firebaseWebApp` Pulumi outputs in agon_infra. */
export const firebaseConfig = {
  apiKey: env.VITE_FIREBASE_API_KEY,
  authDomain: env.VITE_FIREBASE_AUTH_DOMAIN,
  projectId: env.VITE_FIREBASE_PROJECT_ID,
  storageBucket: env.VITE_FIREBASE_STORAGE_BUCKET,
  messagingSenderId: env.VITE_FIREBASE_MESSAGING_SENDER_ID,
  appId: env.VITE_FIREBASE_APP_ID,
}

/** The public half of the Web Push (VAPID) key pair FCM's `getToken()` needs.
 *  Console-only to generate — see agon_infra/index.ts for why. */
export const vapidKey = env.VITE_FIREBASE_VAPID_KEY

/** True if a value is present and isn't an un-substituted `${VAR}` template —
 *  the state runtime-env.ts values are left in if the container's entrypoint
 *  never got that env var (or, in a dev build, if .env just doesn't set it). */
function isSet(value: string | undefined): boolean {
  return !!value && !value.startsWith('${')
}

/**
 * Whether enough Firebase config is present to attempt push registration.
 * Mirrors the backend's "absent config means not configured, not a failure"
 * convention for `AGON_FCM_SERVICE_ACCOUNT_JSON` — until the VAPID key is
 * generated and wired through, push notifications just stay off.
 */
export function isFirebaseConfigured(): boolean {
  return Object.values(firebaseConfig).every(isSet) && isSet(vapidKey)
}

let app: FirebaseApp | undefined

/** Lazily initializes (once) and returns the Firebase app. Only call this
 *  after `isFirebaseConfigured()` — `initializeApp` doesn't validate its
 *  input, so calling it first just moves the failure somewhere more
 *  confusing, inside `getMessaging()`/`getToken()`. */
export function getFirebaseApp(): FirebaseApp {
  if (!app) {
    app = initializeApp(firebaseConfig)
  }
  return app
}

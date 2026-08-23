// The values here will be replaced by set-runtime-env.sh in docker build
const runtimeEnv = {
  VITE_SUPABASE_URL: '${VITE_SUPABASE_URL}',
  VITE_SUPABASE_ANON_KEY: '${VITE_SUPABASE_ANON_KEY}',
  // Firebase Web SDK config for push notifications (see src/lib/firebase.ts).
  // All of these are the public, client-safe identifiers Firebase's own docs
  // ship straight into the bundle — nothing here is a secret. VAPID_KEY is the
  // *public* half of the Web Push key pair (Console → Project Settings →
  // Cloud Messaging → Web configuration), which currently has to be generated
  // by hand; until it's set, push registration stays disabled (see
  // isFirebaseConfigured in src/lib/firebase.ts).
  VITE_FIREBASE_API_KEY: '${VITE_FIREBASE_API_KEY}',
  VITE_FIREBASE_AUTH_DOMAIN: '${VITE_FIREBASE_AUTH_DOMAIN}',
  VITE_FIREBASE_PROJECT_ID: '${VITE_FIREBASE_PROJECT_ID}',
  VITE_FIREBASE_STORAGE_BUCKET: '${VITE_FIREBASE_STORAGE_BUCKET}',
  VITE_FIREBASE_MESSAGING_SENDER_ID: '${VITE_FIREBASE_MESSAGING_SENDER_ID}',
  VITE_FIREBASE_APP_ID: '${VITE_FIREBASE_APP_ID}',
  VITE_FIREBASE_VAPID_KEY: '${VITE_FIREBASE_VAPID_KEY}',
}

type RuntimeEnv = typeof runtimeEnv;

export function getRuntimeEnv(): RuntimeEnv {
  if (import.meta.env.DEV) {
    return {
      VITE_SUPABASE_URL: import.meta.env.VITE_SUPABASE_URL,
      VITE_SUPABASE_ANON_KEY: import.meta.env.VITE_SUPABASE_ANON_KEY,
      VITE_FIREBASE_API_KEY: import.meta.env.VITE_FIREBASE_API_KEY,
      VITE_FIREBASE_AUTH_DOMAIN: import.meta.env.VITE_FIREBASE_AUTH_DOMAIN,
      VITE_FIREBASE_PROJECT_ID: import.meta.env.VITE_FIREBASE_PROJECT_ID,
      VITE_FIREBASE_STORAGE_BUCKET: import.meta.env.VITE_FIREBASE_STORAGE_BUCKET,
      VITE_FIREBASE_MESSAGING_SENDER_ID: import.meta.env.VITE_FIREBASE_MESSAGING_SENDER_ID,
      VITE_FIREBASE_APP_ID: import.meta.env.VITE_FIREBASE_APP_ID,
      VITE_FIREBASE_VAPID_KEY: import.meta.env.VITE_FIREBASE_VAPID_KEY,
    };
  }

  return runtimeEnv;
}


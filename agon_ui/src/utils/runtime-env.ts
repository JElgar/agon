// The values here will be replaced by set-runtime-env.sh in docker build
const runtimeEnv = {
  VITE_SUPABASE_URL: '${VITE_SUPABASE_URL}',
  VITE_SUPABASE_ANON_KEY: '${VITE_SUPABASE_ANON_KEY}',
}

type RuntimeEnv = typeof runtimeEnv;

export function getRuntimeEnv(): RuntimeEnv {
  if (import.meta.env.DEV) {
    return {
      VITE_SUPABASE_URL: import.meta.env.VITE_SUPABASE_URL,
      VITE_SUPABASE_ANON_KEY: import.meta.env.VITE_SUPABASE_ANON_KEY,
    };
  }

  return runtimeEnv;
}


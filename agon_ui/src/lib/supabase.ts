import { getRuntimeEnv } from '@/utils/runtime-env'
import { createClient } from '@supabase/supabase-js'

const env = getRuntimeEnv();

export const supabase = createClient(env.VITE_SUPABASE_URL, env.VITE_SUPABASE_ANON_KEY)

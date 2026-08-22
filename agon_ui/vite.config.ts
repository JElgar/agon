import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'
import path from 'path'
import tailwindcss from "@tailwindcss/vite"

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  // Not VITE_-prefixed on purpose: this only steers this config file's own
  // dev-server proxy, never shipped to client code (unlike VITE_* vars,
  // which Vite exposes in the built bundle). loadEnv (rather than plain
  // process.env) is what actually reads .env for a config-time value like
  // this — Vite only auto-loads .env into process.env for client code.
  const env = loadEnv(mode, __dirname, '')
  const apiProxyTarget = env.AGON_API_PROXY_TARGET || 'https://agon.staging.get-agon.com'

  return {
  plugins: [
    react(),
    tailwindcss(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['favicon.ico', 'apple-touch-icon.png', 'masked-icon.svg'],
      manifest: {
        name: 'Agon App',
        short_name: 'Agon',
        description: 'Team management application',
        theme_color: '#ffffff',
        background_color: '#ffffff',
        display: 'standalone',
        icons: [
          {
            src: 'pwa-192x192.png',
            sizes: '192x192',
            type: 'image/png'
          },
          {
            src: 'pwa-512x512.png',
            sizes: '512x512',
            type: 'image/png'
          }
        ],
        // form_factor: 'wide' is required for the richer install UI on
        // desktop; a screenshot without it (narrow/mobile) is required for
        // the richer install UI on mobile. Chrome needs at least one of each.
        screenshots: [
          {
            src: 'screenshots/desktop-wide.png',
            sizes: '1280x800',
            type: 'image/png',
            form_factor: 'wide',
            label: 'Sign in to Agon on desktop'
          },
          {
            src: 'screenshots/mobile-narrow.png',
            sizes: '390x844',
            type: 'image/png',
            form_factor: 'narrow',
            label: 'Sign in to Agon on mobile'
          }
        ]
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,ico,png,svg}'],
        // `runtime-env-*.js` holds the Supabase URL/key placeholders that the
        // container's entrypoint rewrites at startup (envsubst). It must NOT be
        // precached: workbox would otherwise serve the stale build-time copy from
        // Cache Storage and ignore the runtime-substituted file nginx serves —
        // which is exactly how an old Supabase project URL survived a redeploy.
        // Exclude it from the precache and never let the SW serve a cached copy.
        globIgnores: ['**/runtime-env*.js'],
        // Also exclude /api/* from the SPA navigation fallback: without this,
        // a direct browser navigation to e.g. /api/docs (not in the precache)
        // is intercepted by the service worker and served the app shell
        // instead of hitting the backend, and the app's catch-all route then
        // bounces it to /feed. Only affects navigations (address bar/links),
        // not fetch() calls the already-loaded UI makes to the API.
        navigateFallbackDenylist: [/runtime-env/, /^\/api\//],
        runtimeCaching: [
          {
            urlPattern: /\/assets\/runtime-env.*\.js$/,
            handler: 'NetworkOnly',
          },
        ],
      }
    })
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    // Pin the dev port so it stays stable — the Supabase Redirect URLs allowlist
    // and OAuth `redirectTo` (window.location.origin) must match it exactly, and
    // Vite otherwise drifts to 5174/5175 when 5173 is taken, breaking login.
    port: 5173,
    strictPort: true,
    proxy: {
      // Local dev proxies /api to the staging backend by default, so the UI
      // can run without a local agon_service (which needs
      // DynamoDB/Meilisearch/AWS creds) — auth tokens validate against the
      // same staging Supabase project the UI uses by default. Set
      // AGON_API_PROXY_TARGET=http://localhost:7000 (agon_ui/.env) to point
      // at a local agon_service instead — see agon_ui/e2e/README.md's
      // "Running fully local" section. Staging serves the API under /api
      // (via the ingress) and so does agon_service locally, so the /api
      // prefix is kept either way, not stripped.
      '/api': {
        target: apiProxyTarget,
        changeOrigin: true,
      }
    },
  },
  build: {
    rollupOptions: {
      output: {
        format: 'es',
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
        },
        manualChunks(id) {
          if (/runtime-env.ts/.test(id)) {
            return 'runtime-env'
          }
        },
      },
    }
  },
  }
})

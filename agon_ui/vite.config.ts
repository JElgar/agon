import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'
import path from 'path'
import tailwindcss from "@tailwindcss/vite"

// https://vite.dev/config/
export default defineConfig({
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
        navigateFallbackDenylist: [/runtime-env/],
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
      // Local dev proxies /api to the staging backend, so the UI can run without
      // a local agon_service (which needs DynamoDB/Meilisearch/AWS creds). Auth
      // tokens validate against the same staging Supabase project the UI uses.
      // Staging serves the API under /api (via the ingress), so — unlike a local
      // server at root on :7000 — the /api prefix is kept, not stripped.
      '/api': {
        target: 'https://agon.staging.get-agon.com',
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
})

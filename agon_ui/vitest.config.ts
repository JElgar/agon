import { defineConfig } from 'vite'
import path from 'path'

// Separate from `vite.config.ts` on purpose: the app build needs the React/
// Tailwind/PWA plugins, but the pure-fold unit tests (`cricketFold.test.ts`,
// `footballFold.test.ts`) don't touch React or the DOM at all, so a minimal
// config keeps `npm run test` fast and avoids dragging PWA/service-worker
// plugin setup into a test run that has no use for it. Only the `@` alias
// is shared, since the fold modules import generated API types through it.
export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'node',
  },
})

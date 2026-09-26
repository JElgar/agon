import { useSyncExternalStore } from 'react'

/** Whether a CSS media query currently matches, kept in sync as the window
 *  resizes. Reads the real value on the first render, so a layout that
 *  switches on it doesn't flash the wrong variant first. */
export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const mql = window.matchMedia(query)
      mql.addEventListener('change', onChange)
      return () => mql.removeEventListener('change', onChange)
    },
    () => window.matchMedia(query).matches,
    () => false,
  )
}

/** The desktop tier of the app shell (Tailwind's `xl`, ≥1280px): the fixed
 *  sidebar layout, where pages can use the wide two-column boards. */
export function useIsDesktop(): boolean {
  return useMediaQuery('(min-width: 1280px)')
}

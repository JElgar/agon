import * as React from "react"

const MOBILE_BREAKPOINT = 768
// Tailwind's default `xl` breakpoint — the desktop tier in the redesign
// (fixed sidebar, `DesktopHome.dc.html`), vs. mobile/tablet below it.
const DESKTOP_BREAKPOINT = 1280

export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState<boolean | undefined>(undefined)

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
    const onChange = () => {
      setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    }
    mql.addEventListener("change", onChange)
    setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return !!isMobile
}

/** Whether the viewport is at or above the `xl` desktop breakpoint — for
 *  layouts (like the feed) that restructure rather than just restyle at
 *  that size, where CSS-only responsive classes can't do the job. */
export function useIsDesktop() {
  const [isDesktop, setIsDesktop] = React.useState<boolean | undefined>(undefined)

  React.useEffect(() => {
    const mql = window.matchMedia(`(min-width: ${DESKTOP_BREAKPOINT}px)`)
    const onChange = () => {
      setIsDesktop(window.innerWidth >= DESKTOP_BREAKPOINT)
    }
    mql.addEventListener("change", onChange)
    setIsDesktop(window.innerWidth >= DESKTOP_BREAKPOINT)
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return !!isDesktop
}

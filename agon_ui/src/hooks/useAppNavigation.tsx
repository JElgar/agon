import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  type ReactNode,
} from 'react'
import {
  useLocation,
  useNavigate,
  useNavigationType,
  type NavigateOptions,
} from 'react-router-dom'

interface AppNavContextValue {
  /**
   * Push-navigate to `to`, except when `to` is already somewhere in the
   * stack of routes visited since this provider mounted — then jump
   * straight back to that existing entry (`history.go(-depth)`) instead of
   * pushing a duplicate. Collapses A → B → A → B cycles (e.g. profile →
   * followers → another profile → their followers → back to the first
   * profile) so the stack never grows past the set of *distinct* places
   * visited, and a plain back button is bounded by that.
   */
  goTo: (to: string, options?: NavigateOptions) => void
  /**
   * Go back one step. Falls back to `fallback` (default `/feed`) when
   * there's nothing in our tracked stack to go back to — e.g. the page was
   * opened directly from a deep link (push notification, shared match
   * link, invite link) with no prior in-app navigation, where a raw
   * `history.back()` would just do nothing and look stuck.
   */
  goBack: (fallback?: string) => void
}

export type GoTo = AppNavContextValue['goTo']

const AppNavContext = createContext<AppNavContextValue | null>(null)

function keyFor(pathname: string, search: string): string {
  return pathname + search
}

export function AppNavigationProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const location = useLocation()
  const navigationType = useNavigationType()

  // Our own mirror of the browser history stack, one entry per history
  // entry pushed/replaced/visited since this provider mounted. `goTo` and
  // `goBack` below are the only paths that intentionally move it by more
  // than one entry, and they do so in lockstep with the real history (via
  // `navigate(-depth)`/`navigate(-1)`), so depth computed against this
  // array lines up with what `history.go` will actually do.
  const stackRef = useRef<string[]>([keyFor(location.pathname, location.search)])

  useEffect(() => {
    const key = keyFor(location.pathname, location.search)
    const stack = stackRef.current
    if (navigationType === 'REPLACE') {
      stack[stack.length - 1] = key
      return
    }
    if (navigationType === 'PUSH') {
      if (stack[stack.length - 1] !== key) stack.push(key)
      return
    }
    // POP: either our own goTo/goBack already trimmed the stack to match
    // (nothing to do), or this is a native browser back/forward gesture we
    // didn't initiate. For the latter, snap to the known position if the
    // landing spot is still in our stack, otherwise treat it as a fresh
    // starting point rather than guessing a depth we can't recover.
    if (stack[stack.length - 1] === key) return
    const existingIndex = stack.lastIndexOf(key)
    stackRef.current = existingIndex !== -1 ? stack.slice(0, existingIndex + 1) : [key]
  }, [location.pathname, location.search, navigationType])

  const goTo = useCallback(
    (to: string, options?: NavigateOptions) => {
      if (options?.replace) {
        navigate(to, options)
        return
      }
      const stack = stackRef.current
      const existingIndex = stack.lastIndexOf(to)
      if (existingIndex !== -1 && existingIndex !== stack.length - 1) {
        const depth = stack.length - 1 - existingIndex
        stackRef.current = stack.slice(0, existingIndex + 1)
        navigate(-depth)
        return
      }
      stackRef.current = [...stack, to]
      navigate(to, options)
    },
    [navigate],
  )

  const goBack = useCallback(
    (fallback = '/feed') => {
      const stack = stackRef.current
      if (stack.length <= 1) {
        navigate(fallback, { replace: true })
        return
      }
      stackRef.current = stack.slice(0, -1)
      navigate(-1)
    },
    [navigate],
  )

  return <AppNavContext.Provider value={{ goTo, goBack }}>{children}</AppNavContext.Provider>
}

export function useAppNav(): AppNavContextValue {
  const ctx = useContext(AppNavContext)
  if (!ctx) throw new Error('useAppNav must be used within AppNavigationProvider')
  return ctx
}

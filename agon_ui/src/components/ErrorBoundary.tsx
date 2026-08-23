import { Component, type ReactNode } from 'react'
import { Button } from '@/components/ui/button'

interface Props {
  children: ReactNode
}

interface State {
  hasError: boolean
}

/**
 * Last-resort catch for uncaught render errors. Without this, React 18
 * unmounts the *entire* tree on an uncaught error — the user gets a blank
 * page with nothing but a console trace, which is what "loading the feed
 * occasionally throws a TypeError" looks like from the outside.
 *
 * A common cause in a PWA like this one: `registerType: 'autoUpdate'`
 * (see `vite.config.ts`) updates the service worker in the background, but
 * a tab that's been open across a deploy keeps running the JS it already
 * loaded — now talking to an already-redeployed API whose response shape
 * has moved on. That class of error isn't preventable in general, but it
 * *is* recoverable: reloading picks up the current build. So instead of a
 * blank screen, show a "Reload" prompt.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false }

  static getDerivedStateFromError(): State {
    return { hasError: true }
  }

  componentDidCatch(error: unknown, info: { componentStack: string }) {
    console.error('Uncaught render error:', error, info.componentStack)
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex min-h-screen items-center justify-center">
          <div className="text-center">
            <h2 className="mb-2 text-xl font-semibold">Something went wrong</h2>
            <p className="mb-4 text-muted-foreground">
              Try reloading the page.
            </p>
            <Button onClick={() => window.location.reload()}>Reload</Button>
          </div>
        </div>
      )
    }
    return this.props.children
  }
}

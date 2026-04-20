/**
 * LiquidGL — React wrapper around naughtyduk/liquidGL.
 *
 * Each card-shaped element gets the `liquidGL` class so the global
 * library snapshots the body and renders a refractive WebGL pane.
 * Children are wrapped in `.liquidGL-content` so they render ON TOP of
 * the lens (the library expects a specific child structure).
 */
import { useEffect, useRef, type CSSProperties, type ReactNode } from 'react'

declare global {
  interface Window {
    liquidGL?: {
      (opts: Record<string, unknown>): unknown
      registerDynamic?: (target: string | Element[]) => void
      refresh?: () => void
    }
  }
}

let initialized = false
let pendingInit: ReturnType<typeof setTimeout> | null = null

/**
 * Schedule a (re-)init of liquidGL after current React render flush.
 * The lib walks the DOM for `.liquidGL` targets, so re-init picks up
 * new cards mounted by route changes.
 */
function scheduleInit() {
  if (pendingInit) clearTimeout(pendingInit)
  pendingInit = setTimeout(() => {
    pendingInit = null
    if (typeof window === 'undefined') return
    if (!window.liquidGL) {
      // scripts haven't loaded yet — try again shortly
      pendingInit = setTimeout(scheduleInit, 200)
      return
    }
    try {
      window.liquidGL({
        snapshot: 'body',
        target: '.liquidGL',
        resolution: 2.0,
        refraction: 0.04,
        bevelDepth: 0.12,
        bevelWidth: 0.20,
        frost: 8,           // Heavy frosting per design ask
        shadow: true,
        specular: true,
        reveal: 'fade',
        magnify: 1.0,
      })
      initialized = true
    } catch (e) {
      // If init failed (e.g. WebGL context lost), reset so next mount can retry
      console.warn('liquidGL init failed', e)
      initialized = false
    }
  }, 60)
}

export interface LiquidGLProps {
  children?: ReactNode
  className?: string
  style?: CSSProperties
  /** Extra utility class for the inner content layer (padding, flex, etc) */
  contentClassName?: string
  /** Border radius in px (also applied to the WebGL pane). Default 24. */
  radius?: number
  onClick?: () => void
}

export function LiquidGL({
  children,
  className = '',
  style,
  contentClassName = '',
  radius = 24,
  onClick,
}: LiquidGLProps) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    // Either schedule the first init or refresh existing instance to
    // pick up this new lens.
    if (!initialized) {
      scheduleInit()
    } else if (window.liquidGL?.refresh) {
      // small delay so the new DOM node is mounted
      setTimeout(() => window.liquidGL?.refresh?.(), 30)
    } else {
      // No refresh API — re-init to pick up the new lens
      scheduleInit()
    }
  }, [])

  return (
    <div
      ref={ref}
      onClick={onClick}
      className={`liquidGL ${className}`}
      style={{
        position: 'relative',
        // Explicit z-index creates a stacking context so children render
        // ABOVE the global WebGL canvas the library inserts (which sits at
        // z = maxLensZ - 1, automatically computed from parents).
        zIndex: 1,
        borderRadius: radius,
        ...style,
      }}
    >
      <div
        className={`liquidGL-content ${contentClassName}`}
        style={{ position: 'relative', zIndex: 2, borderRadius: radius }}
      >
        {children}
      </div>
    </div>
  )
}

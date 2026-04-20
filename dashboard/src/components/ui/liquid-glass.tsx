/**
 * Liquid Glass — physics-based refractive surface.
 *
 * Ported from github.com/archisvaze/liquid-glass (Apache-2.0). Uses an SVG
 * `<filter>` with `feImage` + `feDisplacementMap` applied via
 * `backdrop-filter: url(#filter-id)` to actually bend the content behind the
 * card (real refraction at the curved bezel), plus a specular highlight
 * baked into the filter chain.
 *
 * Caveat: SVG `backdrop-filter` is Chromium-only. Safari/Firefox fall back
 * to the regular CSS .glass styling (still nice, just not refractive).
 */
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import type { CSSProperties, ReactNode } from 'react'

type SurfaceFn = (x: number) => number

const SURFACE_FNS: Record<string, SurfaceFn> = {
  convex_squircle: (x) => Math.pow(1 - Math.pow(1 - x, 4), 0.25),
  convex_circle:   (x) => Math.sqrt(1 - (1 - x) * (1 - x)),
  concave:         (x) => 1 - Math.sqrt(1 - (1 - x) * (1 - x)),
  lip: (x) => {
    const convex = Math.pow(1 - Math.pow(1 - Math.min(x * 2, 1), 4), 0.25)
    const concave = 1 - Math.sqrt(1 - (1 - x) * (1 - x)) + 0.1
    const t = 6 * x ** 5 - 15 * x ** 4 + 10 * x ** 3
    return convex * (1 - t) + concave * t
  },
}

function calculateRefractionProfile(
  glassThickness: number,
  bezelWidth: number,
  heightFn: SurfaceFn,
  ior: number,
  samples = 128,
): Float64Array {
  const eta = 1 / ior
  const refract = (nx: number, ny: number): [number, number] | null => {
    const dot = ny
    const k = 1 - eta * eta * (1 - dot * dot)
    if (k < 0) return null
    const sq = Math.sqrt(k)
    return [-(eta * dot + sq) * nx, eta - (eta * dot + sq) * ny]
  }
  const profile = new Float64Array(samples)
  for (let i = 0; i < samples; i++) {
    const x = i / samples
    const y = heightFn(x)
    const dx = x < 1 ? 0.0001 : -0.0001
    const y2 = heightFn(x + dx)
    const deriv = (y2 - y) / dx
    const mag = Math.sqrt(deriv * deriv + 1)
    const ref = refract(-deriv / mag, -1 / mag)
    if (!ref) {
      profile[i] = 0
      continue
    }
    profile[i] = ref[0] * ((y * bezelWidth + glassThickness) / ref[1])
  }
  return profile
}

function generateDisplacementMap(
  w: number, h: number, radius: number, bezelWidth: number,
  profile: Float64Array, maxDisp: number,
): string {
  const c = document.createElement('canvas')
  c.width = w; c.height = h
  const ctx = c.getContext('2d')!
  const img = ctx.createImageData(w, h)
  const d = img.data
  for (let i = 0; i < d.length; i += 4) {
    d[i] = 128; d[i + 1] = 128; d[i + 2] = 0; d[i + 3] = 255
  }
  const r = radius
  const rSq = r * r
  const r1Sq = (r + 1) ** 2
  const rBSq = Math.max(r - bezelWidth, 0) ** 2
  const wB = w - r * 2
  const hB = h - r * 2
  const S = profile.length

  for (let y1 = 0; y1 < h; y1++) {
    for (let x1 = 0; x1 < w; x1++) {
      const x = x1 < r ? x1 - r : x1 >= w - r ? x1 - r - wB : 0
      const y = y1 < r ? y1 - r : y1 >= h - r ? y1 - r - hB : 0
      const dSq = x * x + y * y
      if (dSq > r1Sq || dSq < rBSq) continue
      const dist = Math.sqrt(dSq)
      const fromSide = r - dist
      const op = dSq < rSq ? 1 : 1 - (dist - Math.sqrt(rSq)) / (Math.sqrt(r1Sq) - Math.sqrt(rSq))
      if (op <= 0 || dist === 0) continue
      const cos = x / dist
      const sin = y / dist
      const bi = Math.min(((fromSide / bezelWidth) * S) | 0, S - 1)
      const disp = profile[bi] || 0
      const dX = (-cos * disp) / maxDisp
      const dY = (-sin * disp) / maxDisp
      const idx = (y1 * w + x1) * 4
      d[idx]     = (128 + dX * 127 * op + 0.5) | 0
      d[idx + 1] = (128 + dY * 127 * op + 0.5) | 0
    }
  }
  ctx.putImageData(img, 0, 0)
  return c.toDataURL()
}

function generateSpecularMap(
  w: number, h: number, radius: number, bezelWidth: number,
  angle = Math.PI / 3,
): string {
  const c = document.createElement('canvas')
  c.width = w; c.height = h
  const ctx = c.getContext('2d')!
  const img = ctx.createImageData(w, h)
  const d = img.data
  d.fill(0)
  const r = radius
  const rSq = r * r
  const r1Sq = (r + 1) ** 2
  const rBSq = Math.max(r - bezelWidth, 0) ** 2
  const wB = w - r * 2
  const hB = h - r * 2
  const sv = [Math.cos(angle), Math.sin(angle)]

  for (let y1 = 0; y1 < h; y1++) {
    for (let x1 = 0; x1 < w; x1++) {
      const x = x1 < r ? x1 - r : x1 >= w - r ? x1 - r - wB : 0
      const y = y1 < r ? y1 - r : y1 >= h - r ? y1 - r - hB : 0
      const dSq = x * x + y * y
      if (dSq > r1Sq || dSq < rBSq) continue
      const dist = Math.sqrt(dSq)
      const fromSide = r - dist
      const op = dSq < rSq ? 1 : 1 - (dist - Math.sqrt(rSq)) / (Math.sqrt(r1Sq) - Math.sqrt(rSq))
      if (op <= 0 || dist === 0) continue
      const cos = x / dist
      const sin = -y / dist
      const dot = Math.abs(cos * sv[0] + sin * sv[1])
      const edge = Math.sqrt(Math.max(0, 1 - (1 - fromSide) ** 2))
      const coeff = dot * edge
      const col = (255 * coeff) | 0
      const alpha = (col * coeff * op) | 0
      const idx = (y1 * w + x1) * 4
      d[idx] = col; d[idx + 1] = col; d[idx + 2] = col; d[idx + 3] = alpha
    }
  }
  ctx.putImageData(img, 0, 0)
  return c.toDataURL()
}

export interface LiquidGlassProps {
  children?: ReactNode
  className?: string
  style?: CSSProperties
  /** Border radius in px. Default 24. */
  radius?: number
  /** Glass physical thickness — bigger = more refraction. Default 80. */
  thickness?: number
  /** Bezel width in px (curve at the edge). Default 40. */
  bezel?: number
  /** Index of refraction. Higher = more bending. Default 2.5. */
  ior?: number
  /** Backdrop blur in px. Default 0.4. */
  blur?: number
  /** Specular opacity 0–1. Default 0.5. */
  specularOpacity?: number
  /** Specular saturation 0–12. Default 4. */
  specularSaturation?: number
  /** Surface curvature shape. Default convex_squircle. */
  surface?: keyof typeof SURFACE_FNS
  /** Tint rgba (e.g. 'rgba(255,255,255,0.06)'). */
  tint?: string
  /** Inner shadow color. Default 'rgba(255,255,255,0.45)'. */
  innerShadowColor?: string
  /** Inner shadow blur px. Default 20. */
  innerShadowBlur?: number
  /** Inner shadow spread px (negative = inset). Default -5. */
  innerShadowSpread?: number
  /** Outer drop-shadow blur px. Default 24. */
  outerShadowBlur?: number
  /** click handler — passes through to the root */
  onClick?: () => void
}

export function LiquidGlass({
  children,
  className = '',
  style,
  radius = 24,
  thickness = 80,
  bezel = 40,
  ior = 2.5,
  blur = 0.4,
  specularOpacity = 0.5,
  specularSaturation = 4,
  surface = 'convex_squircle',
  tint = 'rgba(255,255,255,0.06)',
  innerShadowColor = 'rgba(255,255,255,0.45)',
  innerShadowBlur = 20,
  innerShadowSpread = -5,
  outerShadowBlur = 24,
  onClick,
}: LiquidGlassProps) {
  const ref = useRef<HTMLDivElement>(null)
  const filterId = useId().replace(/[:]/g, '_')
  const [size, setSize] = useState<{ w: number; h: number }>({ w: 0, h: 0 })
  const [filterMarkup, setFilterMarkup] = useState<string>('')

  // Watch size
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      const e = entries[0]
      if (!e) return
      const { width, height } = e.contentRect
      setSize((prev) => (prev.w === Math.round(width) && prev.h === Math.round(height) ? prev : { w: Math.round(width), h: Math.round(height) }))
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Rebuild filter when size or params change
  useEffect(() => {
    const { w, h } = size
    if (w < 4 || h < 4) return
    const heightFn = SURFACE_FNS[surface]
    const clampedBezel = Math.min(bezel, radius - 1, Math.min(w, h) / 2 - 1)
    const profile = calculateRefractionProfile(thickness, clampedBezel, heightFn, ior, 128)
    const maxDisp = Math.max(...Array.from(profile).map(Math.abs)) || 1
    const dispUrl = generateDisplacementMap(w, h, radius, clampedBezel, profile, maxDisp)
    const specUrl = generateSpecularMap(w, h, radius, clampedBezel * 2.5)
    const scale = maxDisp
    setFilterMarkup(`
      <filter id="${filterId}" x="0%" y="0%" width="100%" height="100%">
        <feGaussianBlur in="SourceGraphic" stdDeviation="${blur}" result="blurred" />
        <feImage href="${dispUrl}" x="0" y="0" width="${w}" height="${h}" result="dispmap" />
        <feDisplacementMap in="blurred" in2="dispmap" scale="${scale}" xChannelSelector="R" yChannelSelector="G" result="displaced" />
        <feColorMatrix in="displaced" type="saturate" values="${specularSaturation}" result="dispsat" />
        <feImage href="${specUrl}" x="0" y="0" width="${w}" height="${h}" result="speclayer" />
        <feComposite in="dispsat" in2="speclayer" operator="in" result="specmasked" />
        <feComponentTransfer in="speclayer" result="specfaded">
          <feFuncA type="linear" slope="${specularOpacity}" />
        </feComponentTransfer>
        <feBlend in="specmasked" in2="displaced" mode="normal" result="withsat" />
        <feBlend in="specfaded" in2="withsat" mode="normal" />
      </filter>
    `)
  }, [size, radius, thickness, bezel, ior, blur, specularOpacity, specularSaturation, surface, filterId])

  return (
    <div
      ref={ref}
      onClick={onClick}
      style={{
        position: 'relative',
        borderRadius: radius,
        isolation: 'isolate',
        boxShadow: `0 4px ${outerShadowBlur}px rgba(0,0,0,0.28)`,
        ...style,
      }}
    >
      {/* Refractive backdrop layer (under everything) */}
      <div
        aria-hidden
        style={{
          position: 'absolute',
          inset: 0,
          zIndex: 0,
          borderRadius: radius,
          backdropFilter: filterMarkup ? `url(#${filterId})` : 'blur(20px) saturate(180%)',
          WebkitBackdropFilter: filterMarkup ? `url(#${filterId})` : 'blur(20px) saturate(180%)',
          pointerEvents: 'none',
        }}
      />
      {/* Tint + inner highlight layer (above backdrop, below content) */}
      <div
        aria-hidden
        style={{
          position: 'absolute',
          inset: 0,
          zIndex: 1,
          borderRadius: radius,
          background: tint,
          boxShadow: `inset 0 0 ${innerShadowBlur}px ${innerShadowSpread}px ${innerShadowColor}`,
          pointerEvents: 'none',
        }}
      />
      {/* SVG filter definition (one per instance) */}
      {filterMarkup && (
        <svg
          aria-hidden
          width="0"
          height="0"
          style={{ position: 'absolute', overflow: 'hidden', width: 0, height: 0 }}
          // eslint-disable-next-line react/no-danger
          dangerouslySetInnerHTML={{ __html: filterMarkup }}
        />
      )}
      {/* Foreground content layer — receives the user-provided className
       * so flex/grid/padding utilities apply to the actual children
       * container, not the outer positioning wrapper. */}
      <div
        className={className}
        style={{ position: 'relative', zIndex: 2, borderRadius: radius }}
      >
        {children}
      </div>
    </div>
  )
}

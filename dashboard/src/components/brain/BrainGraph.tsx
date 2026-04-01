import { useRef, useCallback, useEffect, useState, useMemo } from 'react'
import ForceGraph2D from 'react-force-graph-2d'
import type { GraphData, GraphNode, GraphLink } from '../../hooks/useApi'

const TYPE_COLORS: Record<string, string> = {
  person: '#00d4ff',
  company: '#863bff',
  service: '#00ffa3',
  concept: '#4d7cff',
  account: '#ffb800',
  project: '#ff3daa',
  location: '#ff6b35',
  event: '#a0ff00',
}

const DEFAULT_COLOR = '#4d7cff'
const LINK_CANVAS_MODE = () => 'replace' as const

function getColor(type: string): string {
  return TYPE_COLORS[type.toLowerCase()] || DEFAULT_COLOR
}

interface Props {
  graph: GraphData | null
  onNodeClick?: (node: GraphNode | null) => void
}

export function BrainGraph({ graph, onNodeClick }: Props) {
  const fgRef = useRef<any>(null) // eslint-disable-line @typescript-eslint/no-explicit-any
  const containerRef = useRef<HTMLDivElement>(null)
  const [dimensions, setDimensions] = useState({ width: 800, height: 600 })
  const [hoveredNode, setHoveredNode] = useState<number | null>(null)
  const [selectedNodeId, setSelectedNodeId] = useState<number | null>(null)

  // Use refs so canvas draw callbacks always read the latest values without
  // causing the ForceGraph to re-initialize when these change.
  const hoveredNodeRef = useRef<number | null>(null)
  hoveredNodeRef.current = hoveredNode

  const selectedNodeIdRef = useRef<number | null>(null)
  selectedNodeIdRef.current = selectedNodeId

  const onNodeClickRef = useRef(onNodeClick)
  onNodeClickRef.current = onNodeClick

  const graphRef = useRef(graph)
  graphRef.current = graph

  // Track container size
  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const obs = new ResizeObserver(([entry]) => {
      setDimensions({ width: entry.contentRect.width, height: entry.contentRect.height })
    })
    obs.observe(el)
    return () => obs.disconnect()
  }, [])

  // Memoize graphData so it only changes when the graph prop changes —
  // NOT on every render (hover / selection state changes).
  const graphData = useMemo(() => {
    if (graph && graph.nodes.length > 0) {
      return {
        nodes: graph.nodes.map((n: GraphNode) => ({ ...n })),
        links: graph.links.map((l: GraphLink) => ({ ...l })),
      }
    }
    return {
      nodes: [{ id: 0, name: 'Mimi', type: 'core', properties: {}, connections: 0 }],
      links: [],
    }
  }, [graph])

  const isEmpty = !graph || graph.nodes.length === 0
  const isEmptyRef = useRef(isEmpty)
  isEmptyRef.current = isEmpty

  // ------- stable canvas callbacks (no deps => never change) -------

  const nodeCanvasObject = useCallback((node: any, ctx: CanvasRenderingContext2D) => {
    const x = node.x as number
    const y = node.y as number
    if (!Number.isFinite(x) || !Number.isFinite(y)) return

    const color = getColor(node.type)
    const empty = isEmptyRef.current
    const size = empty ? 12 : Math.max(4, Math.min(20, 4 + (node.connections || 0) * 2))
    const isHovered = hoveredNodeRef.current === node.id
    const isSelected = selectedNodeIdRef.current === node.id

    // Glow
    const glowAlpha = isSelected ? '80' : isHovered ? '60' : '30'
    const gradient = ctx.createRadialGradient(x, y, 0, x, y, size * 3)
    gradient.addColorStop(0, color + glowAlpha)
    gradient.addColorStop(1, color + '00')
    ctx.beginPath()
    ctx.arc(x, y, size * 3, 0, 2 * Math.PI)
    ctx.fillStyle = gradient
    ctx.fill()

    // Node circle
    ctx.beginPath()
    ctx.arc(x, y, size, 0, 2 * Math.PI)
    ctx.fillStyle = color + (isHovered || isSelected ? 'ff' : 'cc')
    ctx.fill()

    // Selection ring
    if (isSelected) {
      ctx.beginPath()
      ctx.arc(x, y, size + 3, 0, 2 * Math.PI)
      ctx.strokeStyle = color
      ctx.lineWidth = 1.5
      ctx.stroke()
    }

    // Pulsing animation for empty state
    if (empty) {
      const pulse = (Math.sin(Date.now() / 600) + 1) / 2
      const pulseGradient = ctx.createRadialGradient(x, y, size, x, y, size + 20 * pulse)
      pulseGradient.addColorStop(0, color + '20')
      pulseGradient.addColorStop(1, color + '00')
      ctx.beginPath()
      ctx.arc(x, y, size + 20 * pulse, 0, 2 * Math.PI)
      ctx.fillStyle = pulseGradient
      ctx.fill()
    }

    // Label
    if (isHovered || isSelected || empty) {
      ctx.font = `${isHovered || isSelected ? '12' : '10'}px 'Geist Variable', sans-serif`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'top'
      ctx.fillStyle = 'rgba(255,255,255,0.9)'
      ctx.fillText(node.name, x, y + size + 4)
      if (empty) {
        ctx.font = "9px 'Geist Variable', sans-serif"
        ctx.fillStyle = 'rgba(255,255,255,0.35)'
        ctx.fillText('waiting for thoughts...', x, y + size + 18)
      }
    }
  }, [])

  const linkCanvasObject = useCallback((link: any, ctx: CanvasRenderingContext2D) => {
    const source = link.source
    const target = link.target
    if (!source || !target || typeof source.x !== 'number') return

    const hovered = hoveredNodeRef.current
    const selected = selectedNodeIdRef.current
    const isHighlighted =
      hovered === source.id || hovered === target.id ||
      selected === source.id || selected === target.id

    ctx.beginPath()
    ctx.moveTo(source.x, source.y)
    ctx.lineTo(target.x, target.y)
    ctx.strokeStyle = isHighlighted
      ? 'rgba(255,255,255,0.35)'
      : 'rgba(255,255,255,0.08)'
    ctx.lineWidth = isHighlighted ? 1 : 0.5
    ctx.stroke()
  }, [])

  // Stable pointer-area paint (for hit detection on the shadow canvas)
  const nodePointerAreaPaint = useCallback((node: any, color: string, ctx: CanvasRenderingContext2D) => {
    const size = Math.max(4, Math.min(20, 4 + (node.connections || 0) * 2))
    ctx.beginPath()
    ctx.arc(node.x, node.y, size + 6, 0, 2 * Math.PI) // slightly larger hit area
    ctx.fillStyle = color
    ctx.fill()
  }, [])

  // ------- event handlers -------

  const handleNodeClick = useCallback((node: any) => {
    const empty = isEmptyRef.current
    if (empty) return
    const g = graphRef.current
    const gn = g?.nodes.find((n: GraphNode) => n.id === node.id) ?? null
    setSelectedNodeId(gn ? gn.id : null)
    onNodeClickRef.current?.(gn)
  }, [])

  const handleBackgroundClick = useCallback(() => {
    setSelectedNodeId(null)
    onNodeClickRef.current?.(null)
  }, [])

  const handleNodeHover = useCallback((node: any) => {
    setHoveredNode(node?.id ?? null)
  }, [])

  return (
    <div ref={containerRef} className="absolute inset-0 z-0">
      <ForceGraph2D
        ref={fgRef}
        graphData={graphData}
        nodeId="id"
        width={dimensions.width}
        height={dimensions.height}
        backgroundColor="transparent"
        // Keep the render loop alive so hover / selection highlights update
        // even after the force simulation has cooled down.
        autoPauseRedraw={false}
        nodeCanvasObject={nodeCanvasObject}
        nodePointerAreaPaint={nodePointerAreaPaint}
        linkCanvasObjectMode={LINK_CANVAS_MODE}
        linkCanvasObject={linkCanvasObject}
        onNodeHover={handleNodeHover}
        onNodeClick={handleNodeClick}
        onBackgroundClick={handleBackgroundClick}
        enableZoomInteraction={true}
        enablePanInteraction={true}
        cooldownTicks={100}
        d3AlphaDecay={0.02}
        d3VelocityDecay={0.3}
      />
    </div>
  )
}

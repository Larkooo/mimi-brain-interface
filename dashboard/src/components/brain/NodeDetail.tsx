import { X } from 'lucide-react'
import type { GraphNode } from '../../hooks/useApi'

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

export function NodeDetail({ node, onClose }: { node: GraphNode; onClose: () => void }) {
  const color = TYPE_COLORS[node.type.toLowerCase()] || '#4d7cff'
  const props = node.properties && typeof node.properties === 'object'
    ? Object.entries(node.properties)
    : []

  return (
    <div
      className="absolute bottom-20 left-1/2 -translate-x-1/2 glass px-5 py-4 z-20 pointer-events-auto min-w-[280px] max-w-[400px]"
    >
      <button
        onClick={onClose}
        className="absolute top-2 right-2 text-muted-foreground/70 hover:text-foreground/70 transition-colors"
      >
        <X size={14} />
      </button>

      <div className="flex items-center gap-3 mb-3">
        <div
          className="w-3 h-3 rounded-full"
          style={{ backgroundColor: color, boxShadow: `0 0 8px ${color}` }}
        />
        <div>
          <div className="text-sm font-medium text-foreground">{node.name}</div>
          <div className="text-[10px] text-muted-foreground/80 uppercase tracking-wider">{node.type}</div>
        </div>
      </div>

      <div className="flex items-center gap-4 text-xs text-muted-foreground/80 mb-2">
        <span className="font-mono">ID: {node.id}</span>
        <span>{node.connections} connection{node.connections !== 1 ? 's' : ''}</span>
      </div>

      {props.length > 0 && (
        <div className="border-t border-white/6 pt-2 mt-2 space-y-1">
          {props.map(([key, value]) => (
            <div key={key} className="flex gap-2 text-xs">
              <span className="text-muted-foreground/80 font-mono">{key}:</span>
              <span className="text-foreground/85 break-all">{String(value)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

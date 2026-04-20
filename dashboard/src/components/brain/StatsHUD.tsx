import type { Status, BrainStats } from '../../hooks/useApi'

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

export function StatsHUD({ status, stats }: { status: Status | null; stats: BrainStats | null }) {
  return (
    <div className="absolute inset-0 pointer-events-none z-10">
      {/* Top-left: Identity */}
      <div className="absolute top-4 left-4 glass px-4 py-3">
        <div className="flex items-center gap-2">
          <div
            className="w-2 h-2 rounded-full"
            style={{
              backgroundColor: status?.session_running ? '#00ffa3' : '#ff3d3d',
              boxShadow: status?.session_running
                ? '0 0 8px #00ffa3'
                : '0 0 8px #ff3d3d',
              animation: 'pulse 2s ease-in-out infinite',
            }}
          />
          <span className="text-sm font-medium text-foreground">{status?.name || 'Mimi'}</span>
        </div>
        <div className="text-[10px] text-muted-foreground/70 font-mono mt-1">
          {status?.claude_version || 'connecting...'}
        </div>
      </div>

      {/* Top-right: Counts */}
      <div className="absolute top-4 right-4 glass px-4 py-3">
        <div className="flex gap-6">
          <div className="text-center">
            <div className="text-2xl font-mono font-bold text-foreground">
              {stats?.entities ?? '-'}
            </div>
            <div className="text-[10px] text-muted-foreground/70 uppercase tracking-wider">entities</div>
          </div>
          <div className="text-center">
            <div className="text-2xl font-mono font-bold text-[#863bff]">
              {stats?.relationships ?? '-'}
            </div>
            <div className="text-[10px] text-muted-foreground/70 uppercase tracking-wider">links</div>
          </div>
          <div className="text-center">
            <div className="text-2xl font-mono font-bold text-[#00ffa3]">
              {status?.memory_files ?? '-'}
            </div>
            <div className="text-[10px] text-muted-foreground/70 uppercase tracking-wider">memories</div>
          </div>
        </div>
      </div>

      {/* Bottom-left: Entity type legend */}
      {stats && stats.entity_types.length > 0 && (
        <div className="absolute bottom-4 left-4 glass px-4 py-3">
          <div className="text-[10px] text-muted-foreground/70 uppercase tracking-wider mb-2">Entity Types</div>
          <div className="flex flex-col gap-1.5">
            {stats.entity_types.map(([type, count]) => (
              <div key={type} className="flex items-center gap-2">
                <div
                  className="w-2 h-2 rounded-full"
                  style={{ backgroundColor: TYPE_COLORS[type.toLowerCase()] || '#4d7cff' }}
                />
                <span className="text-xs text-foreground/70">{type}</span>
                <span className="text-xs font-mono text-muted-foreground/70">{count}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Bottom-right: Relationship type breakdown */}
      {stats && stats.relationship_types.length > 0 && (
        <div className="absolute bottom-4 right-4 glass px-4 py-3">
          <div className="text-[10px] text-muted-foreground/70 uppercase tracking-wider mb-2">Relationships</div>
          <div className="flex flex-col gap-1.5">
            {stats.relationship_types.map(([type, count]) => (
              <div key={type} className="flex items-center justify-between gap-4">
                <span className="text-xs text-foreground/70">{type}</span>
                <span className="text-xs font-mono text-muted-foreground/80">{count}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.5; }
        }
      `}</style>
    </div>
  )
}

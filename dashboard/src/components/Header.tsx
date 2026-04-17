import type { Status } from '../hooks/useApi'
import { Badge } from '@/components/ui/badge'

export function Header({ status, error }: { status: Status | null; error: string | null }) {
  const name = status?.name ?? 'Mimi'
  const disconnected = !status && !!error

  return (
    <header className="border-b border-border px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="relative">
          <div className={`w-3 h-3 rounded-full ${
            disconnected ? 'bg-amber-500 shadow-[0_0_8px_rgba(245,158,11,0.6)]'
            : status?.session_running ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)]'
            : 'bg-red-500'
          }`} />
        </div>
        <h1 className="text-lg font-semibold tracking-tight">{name}</h1>
        {disconnected ? (
          <Badge variant="outline" className="border-amber-500 text-amber-500">
            disconnected
          </Badge>
        ) : status && (
          <Badge variant={status.session_running ? 'default' : 'destructive'}>
            {status.session_running ? 'online' : 'offline'}
          </Badge>
        )}
      </div>
      <div className="flex items-center gap-4">
        {error && (
          <span className="text-xs text-amber-500 font-mono max-w-[300px] truncate" title={error}>
            API unreachable
          </span>
        )}
        {status && (
          <span className="text-xs text-muted-foreground font-mono">
            {status.claude_version}
          </span>
        )}
      </div>
    </header>
  )
}

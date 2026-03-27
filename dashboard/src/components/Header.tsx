import type { Status } from '../hooks/useApi'
import { Badge } from '@/components/ui/badge'

export function Header({ status }: { status: Status | null }) {
  const name = status?.name ?? 'Mimi'

  return (
    <header className="border-b border-border px-6 py-4 flex items-center justify-between">
      <div className="flex items-center gap-3">
        <div className="relative">
          <div className={`w-3 h-3 rounded-full ${status?.session_running ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)]' : 'bg-red-500'}`} />
        </div>
        <h1 className="text-lg font-semibold tracking-tight">{name}</h1>
        {status && (
          <Badge variant={status.session_running ? 'default' : 'destructive'}>
            {status.session_running ? 'online' : 'offline'}
          </Badge>
        )}
      </div>
      <div className="flex items-center gap-4">
        {status && (
          <span className="text-xs text-muted-foreground font-mono">
            {status.claude_version}
          </span>
        )}
      </div>
    </header>
  )
}

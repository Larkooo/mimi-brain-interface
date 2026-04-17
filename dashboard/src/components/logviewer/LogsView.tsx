import { useEffect, useRef, useState } from 'react'
import type { LogEntry } from '../../hooks/useApi'
import { getLogs, tailLog } from '../../hooks/useApi'
import { Pause, Play, ScrollText } from 'lucide-react'

export function LogsView() {
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [selected, setSelected] = useState<string>('telegram')
  const [content, setContent] = useState<string>('')
  const [autoRefresh, setAutoRefresh] = useState(true)
  const bottomRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    getLogs().then(setLogs).catch(() => {})
  }, [])

  useEffect(() => {
    if (!selected) return
    const load = async () => {
      try {
        const r = await tailLog(selected)
        setContent(r.text)
      } catch { setContent('(failed to load)') }
    }
    load()
    if (!autoRefresh) return
    const t = setInterval(load, 2000)
    return () => clearInterval(t)
  }, [selected, autoRefresh])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'instant' as ScrollBehavior })
  }, [content])

  return (
    <div className="p-8 pl-20 max-w-6xl mx-auto">
      <div className="flex items-baseline justify-between mb-6">
        <div className="flex items-center gap-2">
          <ScrollText size={18} strokeWidth={1.5} className="text-muted-foreground" />
          <h1 className="text-2xl font-semibold tracking-tight">Logs</h1>
        </div>
        <button
          onClick={() => setAutoRefresh(v => !v)}
          className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground px-2 py-1 rounded">
          {autoRefresh ? <Pause size={13} /> : <Play size={13} />}
          {autoRefresh ? 'Pause' : 'Resume'}
        </button>
      </div>

      <div className="flex gap-2 mb-4 overflow-x-auto">
        {logs.map(l => (
          <button key={l.name} onClick={() => setSelected(l.name)}
            className={`px-3 py-1.5 rounded-lg text-xs font-medium whitespace-nowrap transition-colors
              ${selected === l.name
                ? 'bg-foreground text-background'
                : 'bg-card/40 text-muted-foreground hover:text-foreground border border-border/60'}`}>
            {l.name}
            {l.exists && (
              <span className="ml-1.5 opacity-60">
                {(l.size / 1024).toFixed(1)}K
              </span>
            )}
          </button>
        ))}
      </div>

      <div className="rounded-xl border border-border/60 bg-black/40 font-mono text-xs overflow-auto h-[calc(100vh-220px)]">
        <pre className="p-4 whitespace-pre-wrap break-all leading-relaxed">
          {content || '(empty)'}
        </pre>
        <div ref={bottomRef} />
      </div>
    </div>
  )
}

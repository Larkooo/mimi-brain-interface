import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { SubagentSummary, SubagentDetail, SubagentStatus } from '../../hooks/useApi'
import {
  listSubagents,
  getSubagent,
  spawnSubagent,
  sendSubagent,
  stopSubagent,
  deleteSubagent,
} from '../../hooks/useApi'

const STATUS_ICON: Record<SubagentStatus, string> = {
  starting: '◌',
  running: '▶',
  completed: '✓',
  killed: '—',
  failed: '✗',
}

const STATUS_COLOR: Record<SubagentStatus, string> = {
  starting: 'var(--muted-foreground)',
  running: 'var(--accentphosphor)',
  completed: 'var(--accentphosphor)',
  killed: 'var(--muted-foreground)',
  failed: 'var(--danger)',
}

export function SubagentsView() {
  const [agents, setAgents] = useState<SubagentSummary[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)

  const refresh = useCallback(async () => {
    try { setAgents(await listSubagents()) } catch {}
  }, [])

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 5000)
    return () => clearInterval(t)
  }, [refresh])

  const counts = useMemo(() => {
    const c: Record<SubagentStatus, number> = {
      starting: 0, running: 0, completed: 0, killed: 0, failed: 0,
    }
    agents.forEach(a => { c[a.status] = (c[a.status] ?? 0) + 1 })
    return c
  }, [agents])

  return (
    <div className="px-8 pt-10 pb-20 max-w-7xl mx-auto">
      <header className="mb-8">
        <div className="flex items-baseline gap-2 mb-1">
          <span style={{ color: 'var(--accentphosphor)' }}>&gt;</span>
          <h1 className="text-[20px] font-semibold tracking-wide lowercase">subagents</h1>
        </div>
        <p className="text-[13px] text-muted-foreground ml-4">
          long-running claude -p instances. each one stays alive across many turns; send new messages, watch them work.
        </p>
      </header>

      <section className="flex items-center gap-1 mb-6">
        <Chip>all · {agents.length}</Chip>
        <Chip color={STATUS_COLOR.running}>running · {counts.running}</Chip>
        <Chip color={STATUS_COLOR.completed}>done · {counts.completed}</Chip>
        <Chip color={STATUS_COLOR.killed}>killed · {counts.killed}</Chip>
        <Chip color={STATUS_COLOR.failed}>failed · {counts.failed}</Chip>
        <div className="flex-1" />
        <button onClick={() => setCreating(v => !v)} className="term-btn">
          + spawn
        </button>
      </section>

      {creating && (
        <SpawnForm
          onSpawned={async id => {
            setCreating(false)
            await refresh()
            setSelectedId(id)
          }}
          onCancel={() => setCreating(false)}
        />
      )}

      <section className="grid grid-cols-12 gap-4 mt-4">
        <div className="col-span-12 md:col-span-5 term-box">
          <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
            <span style={{ color: 'var(--accentphosphor)' }}>┌─</span>
            <h3 className="text-[12px] font-semibold uppercase tracking-[0.18em]">agents</h3>
            <span className="flex-1 border-t border-border/70 ml-2" />
            <span className="text-[11px] text-muted-foreground num">{agents.length}</span>
          </div>
          <div className="max-h-[78vh] overflow-y-auto">
            {agents.length === 0 && (
              <div className="px-4 py-6 text-[12px] text-muted-foreground">
                # no subagents. click "+ spawn" or run `mimi subagent spawn ...`
              </div>
            )}
            {agents.map(a => (
              <AgentRow
                key={a.id}
                agent={a}
                selected={selectedId === a.id}
                onClick={() => setSelectedId(a.id)}
              />
            ))}
          </div>
        </div>

        <div className="col-span-12 md:col-span-7 term-box">
          {selectedId ? (
            <DetailPanel
              key={selectedId}
              id={selectedId}
              onChanged={refresh}
              onDeleted={() => { setSelectedId(null); refresh() }}
            />
          ) : (
            <div className="px-6 py-10 text-[12px] text-muted-foreground">
              # select an agent to see its meta + live event stream.
            </div>
          )}
        </div>
      </section>
    </div>
  )
}

function Chip({ color, children }: { color?: string; children: React.ReactNode }) {
  return (
    <span
      className="px-2.5 py-1 text-[11px] uppercase tracking-wider"
      style={{
        color: color ?? 'var(--muted-foreground)',
        border: `1px solid ${color ?? 'var(--border)'}`,
      }}
    >
      {children}
    </span>
  )
}

function fmtElapsed(s: number): string {
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m${s % 60 ? ` ${s % 60}s` : ''}`
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return `${h}h${m ? ` ${m}m` : ''}`
}

function AgentRow({ agent, selected, onClick }: { agent: SubagentSummary; selected: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className="w-full text-left px-3 py-2 text-[12px] transition-colors border-b border-border/40"
      style={{
        background: selected ? 'var(--muted)' : 'transparent',
        borderLeft: selected ? `2px solid var(--accentphosphor)` : '2px solid transparent',
      }}
    >
      <div className="flex items-center gap-2">
        <span style={{ color: STATUS_COLOR[agent.status] }}>{STATUS_ICON[agent.status]}</span>
        <span className="truncate flex-1 font-medium">{agent.name}</span>
        <span className="text-[10px] num text-muted-foreground">{fmtElapsed(agent.elapsed_seconds)}</span>
      </div>
      <div className="mt-1 ml-5 text-[10px] text-muted-foreground truncate" title={agent.id}>
        {agent.id}
      </div>
      {agent.last_event_preview && (
        <div className="mt-1 ml-5 text-[11px] text-muted-foreground/80 truncate" title={agent.last_event_preview}>
          <span className="text-[10px] uppercase tracking-wider mr-1.5" style={{ color: 'var(--muted-foreground)' }}>
            {agent.last_event_type}
          </span>
          {agent.last_event_preview}
        </div>
      )}
    </button>
  )
}

interface RenderedEvent {
  ts: number
  ty: string
  body: React.ReactNode
}

function DetailPanel({ id, onChanged, onDeleted }: {
  id: string
  onChanged: () => void
  onDeleted: () => void
}) {
  const [detail, setDetail] = useState<SubagentDetail | null>(null)
  const [events, setEvents] = useState<RenderedEvent[]>([])
  const [statusOverride, setStatusOverride] = useState<{ status?: SubagentStatus; ended_at?: string | null; exit_code?: number | null } | null>(null)
  const [busy, setBusy] = useState(false)
  const [showPrompt, setShowPrompt] = useState(false)
  const eventBoxRef = useRef<HTMLDivElement | null>(null)

  // Initial load: meta + recent events
  useEffect(() => {
    let cancelled = false
    setEvents([])
    setStatusOverride(null)
    setDetail(null)
    setShowPrompt(false)
    getSubagent(id).then(d => {
      if (cancelled) return
      setDetail(d)
      setEvents(d.events.map((e, i) => ({ ts: i, ty: getEventType(e), body: renderEvent(e) })))
    }).catch(() => {})
    return () => { cancelled = true }
  }, [id])

  // SSE stream
  useEffect(() => {
    const url = `/api/subagents/${encodeURIComponent(id)}/events`
    const es = new EventSource(url)
    let mounted = true
    es.addEventListener('event', (e) => {
      if (!mounted) return
      try {
        const v = JSON.parse((e as MessageEvent).data)
        setEvents(prev => {
          const next = [...prev, { ts: Date.now(), ty: getEventType(v), body: renderEvent(v) }]
          // Cap render buffer at 500 events to keep DOM tight.
          return next.length > 500 ? next.slice(next.length - 500) : next
        })
      } catch {}
    })
    es.addEventListener('status', (e) => {
      if (!mounted) return
      try {
        const s = JSON.parse((e as MessageEvent).data)
        setStatusOverride({
          status: s._status,
          ended_at: s._ended_at,
          exit_code: s._exit_code,
        })
      } catch {}
    })
    es.onerror = () => { /* keep open; browser will reconnect */ }
    return () => { mounted = false; es.close() }
  }, [id])

  // Auto-scroll on new events.
  useEffect(() => {
    if (!eventBoxRef.current) return
    const el = eventBoxRef.current
    // Only autoscroll if user is near bottom.
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 200
    if (near) {
      el.scrollTop = el.scrollHeight
    }
  }, [events])

  if (!detail) {
    return <div className="px-6 py-10 text-[12px] text-muted-foreground">loading…</div>
  }

  const meta = detail.meta
  const status: SubagentStatus = (statusOverride?.status ?? meta.status) as SubagentStatus
  const endedAt = statusOverride?.ended_at ?? meta.ended_at
  const exitCode = statusOverride?.exit_code ?? meta.exit_code
  const isRunning = status === 'running' || status === 'starting'

  const onStop = async () => {
    setBusy(true)
    try { await stopSubagent(id); onChanged() } finally { setBusy(false) }
  }
  const onDelete = async () => {
    setBusy(true)
    try { await deleteSubagent(id); onDeleted() } finally { setBusy(false) }
  }

  return (
    <div className="flex flex-col" style={{ maxHeight: '78vh' }}>
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
        <span style={{ color: STATUS_COLOR[status] }}>{STATUS_ICON[status]}</span>
        <h3 className="text-[13px] font-medium truncate flex-1" title={meta.name}>{meta.name}</h3>
        <span className="text-[10px] uppercase tracking-wider num" style={{ color: STATUS_COLOR[status] }}>
          {status}
        </span>
        {isRunning && (
          <button onClick={onStop} disabled={busy} className="term-btn text-[10px]">
            stop
          </button>
        )}
        {!isRunning && (
          <button onClick={onDelete} disabled={busy} className="text-[10px] text-muted-foreground hover:text-danger uppercase tracking-wider">
            delete
          </button>
        )}
      </div>

      <div className="px-4 py-3 grid grid-cols-2 gap-x-6 gap-y-1 text-[11px] border-b border-border">
        <KV k="id"><CopyableMono text={meta.id} /></KV>
        <KV k="model">{meta.model}</KV>
        <KV k="pid">{meta.pid ?? '-'}</KV>
        <KV k="claude_pid">{meta.claude_pid ?? '-'}</KV>
        <KV k="started">{meta.started_at}</KV>
        <KV k="elapsed">{fmtElapsed(detail.elapsed_seconds)}</KV>
        {endedAt && <KV k="ended">{endedAt}</KV>}
        {exitCode != null && <KV k="exit">{exitCode}</KV>}
        <KV k="cwd">
          <span className="truncate" title={meta.cwd}>{meta.cwd}</span>
        </KV>
        {meta.report_channel_id && (
          <KV k="report thread"><CopyableMono text={meta.report_channel_id} /></KV>
        )}
      </div>

      <div className="px-4 py-2 border-b border-border">
        <button
          onClick={() => setShowPrompt(v => !v)}
          className="text-[10px] uppercase tracking-wider text-muted-foreground hover:text-foreground"
        >
          {showPrompt ? '▼' : '▶'} system prompt
        </button>
        {showPrompt && (
          <pre className="mt-2 p-2 bg-muted text-[11px] font-mono whitespace-pre-wrap max-h-48 overflow-auto">
            {meta.system_prompt}
          </pre>
        )}
      </div>

      <div className="flex-1 overflow-hidden flex flex-col">
        <div className="px-4 pt-2 flex items-center gap-2">
          <span className="eyebrow">events</span>
          <span className="flex-1 border-t border-border/40" />
          <span className="text-[10px] num text-muted-foreground">{events.length}</span>
        </div>
        <div ref={eventBoxRef} className="px-4 pb-3 overflow-y-auto flex-1">
          {events.length === 0 && (
            <div className="text-[12px] text-muted-foreground py-3">no events yet — agent may still be initializing.</div>
          )}
          {events.map((e, idx) => (
            <div key={idx} className="py-1 border-b border-border/30 last:border-b-0">
              <div className="flex gap-2 items-baseline">
                <span className="text-[10px] uppercase tracking-wider num" style={{ color: eventColor(e.ty) }}>
                  {e.ty}
                </span>
                <div className="flex-1 text-[12px] font-mono break-words">{e.body}</div>
              </div>
            </div>
          ))}
        </div>
      </div>

      {isRunning && (
        <SendForm id={id} disabled={busy} />
      )}
    </div>
  )
}

function KV({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-muted-foreground text-[10px] lowercase">{k}</span>
      <span className="flex-1 border-b border-dotted border-border mb-1" />
      <span className="num text-[11px] truncate max-w-[60%]">{children}</span>
    </div>
  )
}

function CopyableMono({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <button
      onClick={() => { navigator.clipboard.writeText(text); setCopied(true); setTimeout(() => setCopied(false), 900) }}
      className="font-mono text-[11px] hover:text-accentphosphor"
      title={copied ? 'copied' : 'click to copy'}
    >
      {copied ? '✓ copied' : text}
    </button>
  )
}

function SendForm({ id, disabled }: { id: string; disabled: boolean }) {
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)
  const submit = async () => {
    const t = text.trim()
    if (!t) return
    setSending(true)
    try {
      await sendSubagent(id, t)
      setText('')
    } finally { setSending(false) }
  }
  return (
    <div className="px-4 py-3 border-t border-border bg-muted/30">
      <div className="eyebrow mb-1.5">send message</div>
      <div className="flex gap-2">
        <input
          placeholder="new instruction or follow-up..."
          value={text}
          onChange={e => setText(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit() }}
          disabled={disabled || sending}
          className="flex-1 px-2 py-1.5 bg-background border border-border text-[12px] font-mono"
        />
        <button onClick={submit} disabled={disabled || sending || !text.trim()} className="term-btn">
          {sending ? 'sending…' : 'send'}
        </button>
      </div>
      <div className="mt-1 text-[10px] text-muted-foreground">⌘/ctrl + enter to send</div>
    </div>
  )
}

function SpawnForm({ onSpawned, onCancel }: { onSpawned: (id: string) => void; onCancel: () => void }) {
  const [name, setName] = useState('')
  const [prompt, setPrompt] = useState('')
  const [model, setModel] = useState('')
  const [cwd, setCwd] = useState('')
  const [reportParent, setReportParent] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const submit = async () => {
    if (!name.trim() || !prompt.trim()) { setErr('name + prompt required'); return }
    const rp = reportParent.trim()
    if (rp && !/^\d+$/.test(rp)) {
      setErr('report thread parent must be a numeric Discord channel id')
      return
    }
    setBusy(true); setErr(null)
    try {
      const r = await spawnSubagent({
        name: name.trim(),
        prompt: prompt.trim(),
        model: model.trim() || undefined,
        cwd: cwd.trim() || undefined,
        report_thread_parent: rp || undefined,
      })
      onSpawned(r.id)
    } catch (e) {
      setErr((e as Error).message)
    } finally { setBusy(false) }
  }
  return (
    <div className="term-box p-4 space-y-3 mb-4">
      <div className="flex items-center gap-2">
        <span style={{ color: 'var(--accentphosphor)' }}>$</span>
        <input
          placeholder="name (used in id slug)..."
          value={name}
          autoFocus
          onChange={e => setName(e.target.value)}
          className="flex-1 bg-transparent text-[13px] font-mono outline-none"
        />
      </div>
      <textarea
        placeholder="# system prompt — describes the task; the agent kicks off immediately"
        value={prompt}
        onChange={e => setPrompt(e.target.value)}
        className="w-full px-2 py-1.5 bg-muted border border-border text-[12px] font-mono min-h-[120px]"
      />
      <div className="grid grid-cols-2 gap-3">
        <input
          placeholder="model (default claude-opus-4-7)"
          value={model}
          onChange={e => setModel(e.target.value)}
          className="px-2 py-1.5 bg-muted border border-border text-[12px] font-mono"
        />
        <input
          placeholder="cwd (default ~/.mimi)"
          value={cwd}
          onChange={e => setCwd(e.target.value)}
          className="px-2 py-1.5 bg-muted border border-border text-[12px] font-mono"
        />
      </div>
      <div>
        <input
          placeholder="discord report thread parent channel id (optional)"
          value={reportParent}
          onChange={e => setReportParent(e.target.value)}
          className="w-full px-2 py-1.5 bg-muted border border-border text-[12px] font-mono"
        />
        <div className="mt-1 text-[10px] text-muted-foreground">
          # if set, a public thread is created under this channel and the agent posts progress into it
        </div>
      </div>
      {err && <div className="text-[11px]" style={{ color: 'var(--danger)' }}>{err}</div>}
      <div className="flex gap-2">
        <button className="term-btn" onClick={submit} disabled={busy || !name.trim() || !prompt.trim()}>
          {busy ? 'spawning…' : 'spawn'}
        </button>
        <button className="term-btn" onClick={onCancel}>cancel</button>
      </div>
    </div>
  )
}

// ---------- event rendering ----------

function getEventType(v: any): string { // eslint-disable-line @typescript-eslint/no-explicit-any
  return (v && typeof v === 'object' && typeof v.type === 'string') ? v.type : '?'
}

function eventColor(ty: string): string {
  switch (ty) {
    case 'assistant': return 'var(--accentphosphor)'
    case 'user': return 'var(--muted-foreground)'
    case 'result': return 'var(--accentphosphor)'
    case 'system': return 'var(--muted-foreground)'
    case 'tool_use': return 'var(--accentphosphor)'
    case 'stream_event': return 'var(--muted-foreground)'
    case 'rate_limit_event': return 'var(--warning, #c08a00)'
    default: return 'var(--muted-foreground)'
  }
}

/* eslint-disable @typescript-eslint/no-explicit-any */
function renderEvent(v: any): React.ReactNode {
  if (!v || typeof v !== 'object') return String(v)
  const ty = v.type
  if (ty === 'assistant') {
    const blocks: any[] = v.message?.content ?? []
    return (
      <div className="space-y-1">
        {blocks.map((b, i) => {
          if (b?.type === 'text') {
            return <div key={i} className="whitespace-pre-wrap">{b.text}</div>
          }
          if (b?.type === 'tool_use') {
            const params = b.input ? JSON.stringify(b.input) : ''
            return (
              <div key={i} className="text-[11px]" style={{ color: 'var(--accentphosphor)' }}>
                ⟶ <span className="font-semibold">{b.name}</span>
                {params && (
                  <details className="inline-block ml-2">
                    <summary className="cursor-pointer text-muted-foreground">
                      {truncate(params, 80)}
                    </summary>
                    <pre className="mt-1 p-1.5 bg-muted/50 text-[10px] whitespace-pre-wrap max-w-full overflow-auto">
                      {params}
                    </pre>
                  </details>
                )}
              </div>
            )
          }
          return <div key={i} className="text-[11px] text-muted-foreground">[{b?.type ?? 'unknown'}]</div>
        })}
      </div>
    )
  }
  if (ty === 'user') {
    // tool_result echoes back here.
    const c = v.message?.content
    if (typeof c === 'string') return <div className="whitespace-pre-wrap text-muted-foreground">{c}</div>
    if (Array.isArray(c)) {
      return (
        <div className="space-y-1">
          {c.map((b, i) => {
            const txt = typeof b?.content === 'string' ? b.content
              : typeof b?.text === 'string' ? b.text
              : JSON.stringify(b)
            return (
              <details key={i}>
                <summary className="text-[11px] text-muted-foreground cursor-pointer">
                  ⟵ tool_result · {truncate(String(txt), 80)}
                </summary>
                <pre className="mt-1 p-1.5 bg-muted/50 text-[10px] whitespace-pre-wrap max-w-full overflow-auto">
                  {String(txt)}
                </pre>
              </details>
            )
          })}
        </div>
      )
    }
    return <div className="text-[11px] text-muted-foreground">{JSON.stringify(c)}</div>
  }
  if (ty === 'result') {
    const dur = v.duration_ms ?? 0
    const sub = v.subtype ?? ''
    const turns = v.num_turns ?? 0
    return (
      <span className="text-[11px]">
        turn complete · subtype=<span className="num">{sub}</span> · {dur}ms · {turns} turns
      </span>
    )
  }
  if (ty === 'system') {
    return <span className="text-[11px] text-muted-foreground">system · {v.subtype ?? ''}</span>
  }
  if (ty === 'stream_event') {
    // Heaviest noise — give a one-line collapsible.
    const evt = v.event ?? {}
    const et = evt.type ?? ''
    return <span className="text-[10px] text-muted-foreground">stream · {et}</span>
  }
  return <span className="text-[10px] text-muted-foreground">{truncate(JSON.stringify(v), 200)}</span>
}
/* eslint-enable @typescript-eslint/no-explicit-any */

function truncate(s: string, n: number) {
  return s.length <= n ? s : s.slice(0, n) + '…'
}

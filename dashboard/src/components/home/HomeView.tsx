import { useEffect, useState } from 'react'
import type { Status, ServiceInfo, LogEntry } from '../../hooks/useApi'
import { getServices, getLogs, launchSession, createBackup } from '../../hooks/useApi'
import { Activity, Cpu, Database, Radio, HardDrive, PlayCircle, RotateCcw, Save } from 'lucide-react'

interface Props { status: Status | null }

export function HomeView({ status }: Props) {
  const [services, setServices] = useState<ServiceInfo[]>([])
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)

  useEffect(() => {
    const load = async () => {
      try { setServices(await getServices()) } catch {}
      try { setLogs(await getLogs()) } catch {}
    }
    load()
    const t = setInterval(load, 10000)
    return () => clearInterval(t)
  }, [])

  const flash = (msg: string) => { setToast(msg); setTimeout(() => setToast(null), 2200) }

  const doAction = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(label)
    try { await fn(); flash(`${label} ok`) }
    catch (e) { flash(`${label} failed: ${String(e)}`) }
    finally { setBusy(null) }
  }

  return (
    <div className="p-8 pl-20 max-w-5xl mx-auto">
      <div className="flex items-baseline justify-between mb-8">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Mimi</h1>
          <p className="text-sm text-muted-foreground mt-1">
            {status ? 'online' : '…'} · {status?.claude_version ?? 'unknown'}
          </p>
        </div>
        {toast && <div className="text-xs text-muted-foreground">{toast}</div>}
      </div>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-8">
        <Stat icon={Database} label="Entities" value={status?.brain_stats?.entities ?? 0} />
        <Stat icon={Activity} label="Relationships" value={status?.brain_stats?.relationships ?? 0} />
        <Stat icon={Radio} label="Channels" value={status?.channels?.length ?? 0} />
        <Stat icon={Cpu} label="Memory refs" value={status?.brain_stats?.memory_refs ?? 0} />
      </div>

      <div className="grid md:grid-cols-2 gap-6">
        <Section title="Services" icon={HardDrive}>
          {services.length === 0 && (
            <div className="text-xs text-muted-foreground py-4">No services reporting.</div>
          )}
          {services.map(s => (
            <div key={s.name} className="flex items-center justify-between py-2 border-b border-border/40 last:border-0">
              <div>
                <div className="text-sm font-medium">{s.name}</div>
                <div className="text-xs text-muted-foreground">
                  <StateDot state={s.active_state} /> {s.active_state} ({s.sub_state})
                  {s.main_pid ? ` · pid ${s.main_pid}` : ''}
                </div>
              </div>
            </div>
          ))}
        </Section>

        <Section title="Quick actions" icon={PlayCircle}>
          <ActionRow label="Relaunch Mimi (tmux)" hint="Kills tmux session and starts fresh"
            busy={busy === 'launch'} icon={RotateCcw}
            onClick={() => doAction('launch', launchSession)} />
          <ActionRow label="Back up ~/.mimi" hint="Writes to ~/.mimi/backups/"
            busy={busy === 'backup'} icon={Save}
            onClick={() => doAction('backup', createBackup)} />
        </Section>

        <Section title="Logs" icon={Activity}>
          {logs.length === 0 && (
            <div className="text-xs text-muted-foreground py-4">No log files.</div>
          )}
          {logs.map(l => (
            <div key={l.name} className="flex items-center justify-between py-1.5 text-sm">
              <span className="font-medium">{l.name}</span>
              <span className="text-xs text-muted-foreground">
                {l.exists ? `${(l.size / 1024).toFixed(1)} KB` : 'missing'}
              </span>
            </div>
          ))}
        </Section>

        <Section title="Model" icon={Cpu}>
          <div className="text-sm text-muted-foreground">
            <div className="py-1.5 flex justify-between">
              <span>Session</span><span className="text-foreground">{status?.session_name ?? '—'}</span>
            </div>
            <div className="py-1.5 flex justify-between">
              <span>Model</span><span className="text-foreground">{status?.model ?? '—'}</span>
            </div>
            <div className="py-1.5 flex justify-between">
              <span>Dashboard port</span><span className="text-foreground">{status?.dashboard_port ?? '—'}</span>
            </div>
          </div>
        </Section>
      </div>
    </div>
  )
}

function Stat({ icon: Icon, label, value }: { icon: React.ElementType; label: string; value: number | string }) {
  return (
    <div className="rounded-xl border border-border/60 bg-card/40 backdrop-blur px-4 py-3">
      <div className="flex items-center gap-2 text-xs text-muted-foreground mb-1">
        <Icon size={14} strokeWidth={1.5} />
        <span>{label}</span>
      </div>
      <div className="text-2xl font-semibold tabular-nums">{value}</div>
    </div>
  )
}

function Section({ title, icon: Icon, children }: { title: string; icon: React.ElementType; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border/60 bg-card/40 backdrop-blur p-5">
      <div className="flex items-center gap-2 mb-3">
        <Icon size={15} strokeWidth={1.5} className="text-muted-foreground" />
        <h3 className="text-sm font-medium">{title}</h3>
      </div>
      <div>{children}</div>
    </div>
  )
}

function ActionRow({ label, hint, busy, icon: Icon, onClick }: { label: string; hint: string; busy: boolean; icon: React.ElementType; onClick: () => void }) {
  return (
    <button onClick={onClick} disabled={busy}
      className="w-full text-left py-2.5 px-3 -mx-3 rounded-lg hover:bg-accent/50 disabled:opacity-50 transition-colors group">
      <div className="flex items-center justify-between">
        <div>
          <div className="text-sm font-medium">{label}</div>
          <div className="text-xs text-muted-foreground">{hint}</div>
        </div>
        <Icon size={16} strokeWidth={1.5} className="text-muted-foreground group-hover:text-foreground" />
      </div>
    </button>
  )
}

function StateDot({ state }: { state: string }) {
  const color = state === 'active' ? 'bg-emerald-500' : state === 'failed' ? 'bg-red-500' : 'bg-zinc-500'
  return <span className={`inline-block w-1.5 h-1.5 rounded-full mr-1 ${color}`} />
}

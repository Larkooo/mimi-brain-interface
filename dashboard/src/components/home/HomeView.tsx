import { useEffect, useState } from 'react'
import type { Status, ServiceInfo, LogEntry } from '../../hooks/useApi'
import { getServices, getLogs, launchSession, createBackup } from '../../hooks/useApi'
import { Activity, Cpu, Database, Radio, HardDrive, RotateCcw, Save, ArrowRight } from 'lucide-react'
import { LiquidGL } from '@/components/ui/liquid-gl'

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

  const online = !!status

  return (
    <div className="px-8 pt-20 pb-12 max-w-5xl mx-auto">
      {/* Hero */}
      <section className="relative mb-14">
        <div className="flex items-center gap-2 mb-5">
          <span
            className={`w-1.5 h-1.5 rounded-full ${online ? 'bg-success' : 'bg-danger'}`}
            style={{ boxShadow: online ? '0 0 12px var(--success)' : '0 0 12px var(--danger)' }}
          />
          <span className="eyebrow">{online ? 'systems online' : 'offline'}</span>
          {toast && <span className="text-[11px] text-muted-foreground ml-3 normal-case tracking-normal">{toast}</span>}
        </div>

        <h1
          className="text-[72px] leading-[0.95] font-semibold tracking-[-0.03em] max-w-3xl"
          style={{
            backgroundImage:
              'linear-gradient(120deg, rgb(245,245,247) 0%, rgb(245,245,247) 60%, rgb(180,170,240) 100%)',
            WebkitBackgroundClip: 'text',
            WebkitTextFillColor: 'transparent',
            backgroundClip: 'text',
          }}
        >
          Hey, I'm Mimi.
        </h1>
        <p className="text-[17px] text-muted-foreground mt-5 max-w-xl leading-relaxed">
          A persistent brain with her own memory, identity, and channels.
          Running on Claude {status?.claude_version ?? '…'}.
        </p>
      </section>

      {/* Stats */}
      <section className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-10">
        <Stat icon={Database}  label="Entities"      value={status?.brain_stats?.entities ?? 0} />
        <Stat icon={Activity}  label="Relationships" value={status?.brain_stats?.relationships ?? 0} />
        <Stat icon={Radio}     label="Channels"      value={status?.channels?.length ?? 0} />
        <Stat icon={Cpu}       label="Memory refs"   value={status?.brain_stats?.memory_refs ?? 0} />
      </section>

      {/* Two-column glass cards */}
      <section className="grid md:grid-cols-2 gap-4">
        <Panel title="Services" icon={HardDrive}>
          {services.length === 0 && <Empty>No services reporting.</Empty>}
          {services.map(s => (
            <div key={s.name} className="flex items-center justify-between py-3 border-b border-white/8 last:border-0">
              <div className="min-w-0">
                <div className="text-[13px] font-medium tracking-tight truncate">{s.name}</div>
                <div className="text-[11px] text-muted-foreground mt-0.5 flex items-center gap-1.5">
                  <StateDot state={s.active_state} />
                  <span>{s.active_state}</span>
                  <span className="text-foreground/30">·</span>
                  <span>{s.sub_state}</span>
                  {s.main_pid ? <><span className="text-foreground/30">·</span><span className="num">pid {s.main_pid}</span></> : null}
                </div>
              </div>
            </div>
          ))}
        </Panel>

        <Panel title="Quick actions" icon={ArrowRight}>
          <ActionRow label="Relaunch Mimi" hint="Recreates the tmux session"
            busy={busy === 'launch'} icon={RotateCcw}
            onClick={() => doAction('launch', launchSession)} />
          <ActionRow label="Back up brain" hint="Snapshots ~/.mimi to ~/.mimi/backups/"
            busy={busy === 'backup'} icon={Save}
            onClick={() => doAction('backup', createBackup)} />
        </Panel>

        <Panel title="Logs" icon={Activity}>
          {logs.length === 0 && <Empty>No log files.</Empty>}
          {logs.map(l => (
            <div key={l.name} className="flex items-center justify-between py-2 text-[13px]">
              <span className="font-medium tracking-tight">{l.name}</span>
              <span className="text-[11px] text-muted-foreground num">
                {l.exists ? `${(l.size / 1024).toFixed(1)} KB` : 'missing'}
              </span>
            </div>
          ))}
        </Panel>

        <Panel title="Identity" icon={Cpu}>
          <Row k="Session"        v={status?.session_name ?? '—'} mono />
          <Row k="Model"          v={status?.model ?? '—'} mono />
          <Row k="Dashboard port" v={status?.dashboard_port?.toString() ?? '—'} mono />
        </Panel>
      </section>
    </div>
  )
}

function Stat({ icon: Icon, label, value }: { icon: React.ElementType; label: string; value: number | string }) {
  return (
    <LiquidGL radius={22} contentClassName="px-5 py-4">
      <div className="flex items-center gap-1.5 text-[11px] text-foreground/65 mb-2">
        <Icon size={13} strokeWidth={1.6} />
        <span className="tracking-tight">{label}</span>
      </div>
      <div className="text-[34px] font-semibold leading-none tracking-tight num text-foreground">{value}</div>
    </LiquidGL>
  )
}

function Panel({ title, icon: Icon, children }: { title: string; icon: React.ElementType; children: React.ReactNode }) {
  return (
    <LiquidGL radius={26} contentClassName="p-5">
      <div className="flex items-center gap-2 mb-3">
        <Icon size={14} strokeWidth={1.6} className="text-foreground/65" />
        <h3 className="text-[13px] font-medium tracking-tight text-foreground">{title}</h3>
      </div>
      <div>{children}</div>
    </LiquidGL>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return <div className="text-[12px] text-muted-foreground py-3">{children}</div>
}

function Row({ k, v, mono }: { k: string; v: string; mono?: boolean }) {
  return (
    <div className="flex items-center justify-between py-1.5 text-[13px]">
      <span className="text-muted-foreground tracking-tight">{k}</span>
      <span className={mono ? 'font-mono text-[12px]' : ''}>{v}</span>
    </div>
  )
}

function ActionRow({ label, hint, busy, icon: Icon, onClick }: { label: string; hint: string; busy: boolean; icon: React.ElementType; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      disabled={busy}
      className="w-full text-left py-3 px-3 -mx-3 rounded-xl disabled:opacity-50 transition-colors group"
      style={{ background: 'transparent' }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'rgba(255,255,255,0.06)' }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent' }}
    >
      <div className="flex items-center justify-between">
        <div>
          <div className="text-[13px] font-medium tracking-tight">{label}</div>
          <div className="text-[11px] text-muted-foreground">{hint}</div>
        </div>
        <Icon size={15} strokeWidth={1.6} className="text-muted-foreground group-hover:text-foreground transition-colors" />
      </div>
    </button>
  )
}

function StateDot({ state }: { state: string }) {
  const cls = state === 'active' ? 'bg-success' : state === 'failed' ? 'bg-danger' : 'bg-muted-foreground/60'
  const glow = state === 'active' ? '0 0 8px var(--success)' : state === 'failed' ? '0 0 8px var(--danger)' : 'none'
  return <span className={`inline-block w-1.5 h-1.5 rounded-full ${cls}`} style={{ boxShadow: glow }} />
}

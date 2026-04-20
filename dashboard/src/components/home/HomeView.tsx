import { useEffect, useState } from 'react'
import type { Status, ServiceInfo, LogEntry } from '../../hooks/useApi'
import { getServices, getLogs, launchSession, createBackup } from '../../hooks/useApi'

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
    <div className="px-8 pt-10 pb-20 max-w-6xl mx-auto">
      {/* Boot line — terminal-style opening */}
      <div className="mb-8">
        <div className="text-[12px] text-muted-foreground">
          <span style={{ color: 'var(--accentphosphor)' }}>&gt;</span>{' '}
          mimi.init <span className="mx-1">··</span> claude {status?.claude_version ?? '-'} <span className="mx-1">··</span>{' '}
          <span style={{ color: online ? 'var(--success)' : 'var(--danger)' }}>
            [{online ? ' ok ' : 'fail'}]
          </span>
          {toast && <span className="ml-3 text-muted-foreground/70"># {toast}</span>}
        </div>
        <h1 className="mt-6 text-[44px] font-semibold leading-none tracking-tight lowercase">
          hey, i'm mimi.
        </h1>
        <p className="mt-4 text-[14px] text-muted-foreground max-w-xl">
          a persistent brain with her own memory, identity, and channels.
        </p>
      </div>

      {/* Stats grid — terminal boxes */}
      <section className="grid grid-cols-2 md:grid-cols-4 gap-0 border border-border mb-10">
        <Stat label="entities"      value={status?.brain_stats?.entities ?? 0}      borderR />
        <Stat label="relationships" value={status?.brain_stats?.relationships ?? 0} borderR />
        <Stat label="channels"      value={status?.channels?.length ?? 0}           borderR />
        <Stat label="memory refs"   value={status?.brain_stats?.memory_refs ?? 0} />
      </section>

      {/* Two-column panels */}
      <section className="grid md:grid-cols-2 gap-6 mb-10">
        <Panel title="services">
          {services.length === 0 && <Empty>no services reporting.</Empty>}
          {services.map((s, i) => (
            <div
              key={s.name}
              className="flex items-center justify-between px-4 py-3"
              style={{ borderTop: i === 0 ? 'none' : '1px solid var(--border)' }}
            >
              <div className="min-w-0">
                <div className="text-[13px] lowercase">{s.name}</div>
                <div className="text-[11px] text-muted-foreground mt-0.5">
                  {s.active_state} · {s.sub_state}{s.main_pid ? ` · pid ${s.main_pid}` : ''}
                </div>
              </div>
              <StatusTag state={s.active_state} />
            </div>
          ))}
        </Panel>

        <Panel title="actions">
          <ActionRow
            label="relaunch mimi"
            hint="recreates the tmux session"
            busy={busy === 'launch'}
            onClick={() => doAction('launch', launchSession)}
          />
          <ActionRow
            label="back up brain"
            hint="snapshots ~/.mimi to ~/.mimi/backups/"
            busy={busy === 'backup'}
            onClick={() => doAction('backup', createBackup)}
          />
        </Panel>
      </section>

      {/* Logs + Identity */}
      <section className="grid md:grid-cols-2 gap-6">
        <Panel title="logs">
          {logs.length === 0 && <Empty>no log files.</Empty>}
          {logs.map((l, i) => (
            <div
              key={l.name}
              className="flex items-center justify-between px-4 py-2 text-[13px]"
              style={{ borderTop: i === 0 ? 'none' : '1px solid var(--border)' }}
            >
              <span className="lowercase">{l.name}</span>
              <span className="text-[11px] text-muted-foreground num">
                {l.exists ? `${(l.size / 1024).toFixed(1)} KB` : '-'}
              </span>
            </div>
          ))}
        </Panel>

        <Panel title="identity">
          <div className="px-4 py-3 space-y-2">
            <KV k="session" v={status?.session_name ?? '-'} />
            <KV k="model" v={status?.model ?? '-'} />
            <KV k="port" v={status?.dashboard_port?.toString() ?? '-'} />
          </div>
        </Panel>
      </section>
    </div>
  )
}

function Stat({ label, value, borderR }: { label: string; value: number | string; borderR?: boolean }) {
  return (
    <div
      className="px-5 py-5"
      style={{
        borderRight: borderR ? '1px solid var(--border)' : undefined,
      }}
    >
      <div className="text-[11px] text-muted-foreground uppercase tracking-[0.18em] mb-2">{label}</div>
      <div className="text-[30px] font-semibold leading-none num">{value}</div>
    </div>
  )
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="term-box">
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
        <span style={{ color: 'var(--accentphosphor)' }}>┌─</span>
        <h3 className="text-[12px] font-semibold uppercase tracking-[0.18em]">{title}</h3>
        <span className="flex-1 border-t border-border/70 ml-2"></span>
      </div>
      <div>{children}</div>
    </div>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-4 py-6 text-[12px] text-muted-foreground">
      # {children}
    </div>
  )
}

function KV({ k, v }: { k: string; v: string }) {
  return (
    <div className="term-kv">
      <span className="k">{k}</span>
      <span className="dots" />
      <span className="v">{v}</span>
    </div>
  )
}

function ActionRow({ label, hint, busy, onClick }: { label: string; hint: string; busy: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      disabled={busy}
      className="w-full text-left px-4 py-3 transition-colors disabled:opacity-50"
      style={{ borderTop: '1px solid var(--border)' }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--muted)' }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent' }}
    >
      <div className="flex items-center justify-between">
        <div>
          <div className="text-[13px] lowercase flex items-center gap-2">
            <span style={{ color: 'var(--accentphosphor)' }}>$</span>
            {label}
          </div>
          <div className="text-[11px] text-muted-foreground mt-0.5"># {hint}</div>
        </div>
        {busy && <span className="term-tag">running</span>}
      </div>
    </button>
  )
}

function StatusTag({ state }: { state: string }) {
  if (state === 'active')  return <span className="term-tag ok">ok</span>
  if (state === 'failed')  return <span className="term-tag fail">fail</span>
  return <span className="term-tag">{state}</span>
}

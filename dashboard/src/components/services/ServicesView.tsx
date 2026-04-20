import { useEffect, useState } from 'react'
import type { ServiceInfo } from '../../hooks/useApi'
import { getServices, restartService, startService, stopService } from '../../hooks/useApi'
import { HardDrive, Play, Square, RotateCcw } from 'lucide-react'

export function ServicesView() {
  const [services, setServices] = useState<ServiceInfo[]>([])
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)

  const refresh = async () => {
    try { setServices(await getServices()) } catch {}
  }

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 5000)
    return () => clearInterval(t)
  }, [])

  const flash = (m: string) => { setToast(m); setTimeout(() => setToast(null), 2500) }

  const act = async (name: string, kind: 'start' | 'stop' | 'restart') => {
    setBusy(`${kind}:${name}`)
    try {
      if (kind === 'start') await startService(name)
      else if (kind === 'stop') await stopService(name)
      else await restartService(name)
      flash(`${kind} ${name}`)
      setTimeout(refresh, 500)
    } catch (e) { flash(`${kind} ${name} failed: ${String(e)}`) }
    finally { setBusy(null) }
  }

  return (
    <div className="px-8 pt-20 pb-8 max-w-4xl mx-auto">
      <div className="flex items-baseline justify-between mb-8">
        <div className="flex items-center gap-2">
          <HardDrive size={18} strokeWidth={1.5} className="text-muted-foreground" />
          <h1 className="text-2xl font-semibold tracking-tight">Services</h1>
        </div>
        {toast && <div className="text-xs text-muted-foreground">{toast}</div>}
      </div>

      <div className="grid gap-3">
        {services.length === 0 && (
          <div className="rounded-xl border border-border/60 bg-card/40 p-6 text-sm text-muted-foreground">
            No managed services. Add one by creating a file in <code className="text-foreground">~/.config/systemd/user/</code>.
          </div>
        )}
        {services.map(s => (
          <div key={s.name} className="rounded-xl border border-border/60 bg-card/40 backdrop-blur p-5">
            <div className="flex items-start justify-between mb-3">
              <div>
                <div className="flex items-center gap-2 mb-1">
                  <StateDot state={s.active_state} />
                  <h3 className="text-base font-medium">{s.name}</h3>
                </div>
                <div className="text-xs text-muted-foreground space-y-0.5">
                  <div>{s.active_state} · {s.sub_state}{s.main_pid ? ` · pid ${s.main_pid}` : ''}</div>
                  <div>{s.enabled ? 'enabled on boot' : 'disabled'}</div>
                </div>
              </div>
              <div className="flex gap-1">
                <IconBtn label="Start" icon={Play} onClick={() => act(s.name, 'start')}
                  disabled={!!busy || s.active_state === 'active'} />
                <IconBtn label="Restart" icon={RotateCcw} onClick={() => act(s.name, 'restart')}
                  disabled={!!busy} />
                <IconBtn label="Stop" icon={Square} onClick={() => act(s.name, 'stop')}
                  disabled={!!busy || s.active_state !== 'active'} danger />
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

function IconBtn({ icon: Icon, label, onClick, disabled, danger }: {
  icon: React.ElementType; label: string; onClick: () => void; disabled?: boolean; danger?: boolean
}) {
  return (
    <button onClick={onClick} disabled={disabled} title={label}
      className={`w-8 h-8 rounded-lg flex items-center justify-center transition-colors
        ${disabled ? 'opacity-30 cursor-not-allowed' : 'hover:bg-accent'}
        ${danger ? 'hover:text-red-500' : ''}`}>
      <Icon size={14} strokeWidth={1.5} />
    </button>
  )
}

function StateDot({ state }: { state: string }) {
  const color = state === 'active' ? 'bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.6)]'
    : state === 'failed' ? 'bg-red-500'
    : 'bg-zinc-500'
  return <span className={`inline-block w-2 h-2 rounded-full ${color}`} />
}

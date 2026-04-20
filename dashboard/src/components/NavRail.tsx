import { Brain, Radio, Clock, KeyRound, Settings, BookOpen, Home, ScrollText, HardDrive, Flame } from 'lucide-react'

export type View = 'home' | 'brain' | 'memory' | 'channels' | 'crons' | 'secrets' | 'logs' | 'services' | 'nutrition' | 'settings'

const navItems: { id: View; icon: typeof Brain; label: string }[] = [
  { id: 'home', icon: Home, label: 'Overview' },
  { id: 'brain', icon: Brain, label: 'Knowledge' },
  { id: 'memory', icon: BookOpen, label: 'Memory' },
  { id: 'channels', icon: Radio, label: 'Channels' },
  { id: 'crons', icon: Clock, label: 'Schedules' },
  { id: 'nutrition', icon: Flame, label: 'Nutrition' },
  { id: 'services', icon: HardDrive, label: 'Services' },
  { id: 'logs', icon: ScrollText, label: 'Logs' },
  { id: 'secrets', icon: KeyRound, label: 'Secrets' },
  { id: 'settings', icon: Settings, label: 'Settings' },
]

export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <nav className="fixed left-0 top-0 bottom-0 w-[220px] flex flex-col z-50 bg-sidebar/85 backdrop-blur-xl border-r border-sidebar-border">
      <div className="px-5 pt-6 pb-5 flex items-center gap-2.5">
        <div
          className="w-7 h-7 rounded-md grid place-items-center"
          style={{
            background: 'linear-gradient(135deg, var(--brand), color-mix(in oklch, var(--brand) 50%, transparent))',
            boxShadow: '0 0 0 1px color-mix(in oklch, var(--brand) 30%, transparent), 0 8px 24px -10px var(--brand)',
          }}
        >
          <span className="text-[12px] font-semibold text-background tracking-tight">M</span>
        </div>
        <div className="leading-tight">
          <div className="text-[13px] font-semibold tracking-tight text-foreground">mimi</div>
          <div className="text-[10px] text-muted-foreground">brain interface</div>
        </div>
      </div>

      <div className="px-2.5 flex-1 overflow-y-auto">
        <div className="eyebrow px-2 pb-2 pt-2">Workspace</div>
        <div className="flex flex-col gap-0.5">
          {navItems.map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              onClick={() => onChange(id)}
              className={[
                'group flex items-center gap-2.5 px-2.5 py-1.5 rounded-md text-[13px] transition-colors',
                active === id
                  ? 'bg-sidebar-accent text-foreground'
                  : 'text-muted-foreground hover:bg-sidebar-accent/60 hover:text-foreground',
              ].join(' ')}
            >
              <Icon
                size={15}
                strokeWidth={active === id ? 2 : 1.6}
                className={active === id ? 'text-foreground' : 'text-muted-foreground group-hover:text-foreground'}
              />
              <span className="font-medium tracking-tight">{label}</span>
              {active === id && (
                <span
                  className="ml-auto w-1 h-1 rounded-full"
                  style={{ background: 'var(--brand)', boxShadow: '0 0 8px var(--brand)' }}
                />
              )}
            </button>
          ))}
        </div>
      </div>

      <div className="px-5 py-4 text-[10px] text-muted-foreground/70 border-t border-sidebar-border">
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-success" style={{ boxShadow: '0 0 6px var(--success)' }} />
          <span>online</span>
          <span className="ml-auto num">v1</span>
        </div>
      </div>
    </nav>
  )
}

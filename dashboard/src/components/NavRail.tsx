import { Brain, Radio, Clock, KeyRound, Settings, BookOpen, Home, ScrollText, HardDrive, Flame } from 'lucide-react'
import { LiquidGL } from '@/components/ui/liquid-gl'

export type View = 'home' | 'brain' | 'memory' | 'channels' | 'crons' | 'secrets' | 'logs' | 'services' | 'nutrition' | 'settings'

const navItems: { id: View; icon: typeof Brain; label: string }[] = [
  { id: 'home',      icon: Home,       label: 'Overview' },
  { id: 'brain',     icon: Brain,      label: 'Knowledge' },
  { id: 'memory',    icon: BookOpen,   label: 'Memory' },
  { id: 'channels',  icon: Radio,      label: 'Channels' },
  { id: 'crons',     icon: Clock,      label: 'Schedules' },
  { id: 'nutrition', icon: Flame,      label: 'Nutrition' },
  { id: 'services',  icon: HardDrive,  label: 'Services' },
  { id: 'logs',      icon: ScrollText, label: 'Logs' },
  { id: 'secrets',   icon: KeyRound,   label: 'Secrets' },
  { id: 'settings',  icon: Settings,   label: 'Settings' },
]

export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <div className="fixed left-1/2 -translate-x-1/2 bottom-6 z-50">
      <LiquidGL radius={9999} contentClassName="px-2 py-2 flex items-center gap-1">
        {navItems.map(({ id, icon: Icon, label }) => {
          const isActive = active === id
          return (
            <button
              key={id}
              onClick={() => onChange(id)}
              title={label}
              className="relative grid place-items-center w-10 h-10 rounded-full transition-all duration-200 group"
              style={
                isActive
                  ? {
                      background:
                        'linear-gradient(180deg, rgba(255,255,255,0.32), rgba(255,255,255,0.12))',
                      boxShadow:
                        'inset 0 1px 0 0 rgba(255,255,255,0.45), 0 4px 12px -4px rgba(0,0,0,0.5)',
                    }
                  : undefined
              }
            >
              <Icon
                size={17}
                strokeWidth={isActive ? 2.2 : 1.6}
                className={isActive ? 'text-white' : 'text-white/65 group-hover:text-white transition-colors'}
              />
              <span
                className="pointer-events-none absolute -top-9 left-1/2 -translate-x-1/2 px-2 py-1 rounded-md text-[10px] tracking-wide whitespace-nowrap opacity-0 group-hover:opacity-100 transition-opacity duration-150"
                style={{
                  background: 'rgba(0,0,0,0.6)',
                  backdropFilter: 'blur(20px)',
                  color: '#fff',
                }}
              >
                {label}
              </span>
            </button>
          )
        })}
      </LiquidGL>
    </div>
  )
}

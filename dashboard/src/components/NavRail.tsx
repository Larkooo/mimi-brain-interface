import { Brain, Radio, Clock, KeyRound, Settings, BookOpen, Home, ScrollText, HardDrive, Flame } from 'lucide-react'

export type View = 'home' | 'brain' | 'memory' | 'channels' | 'crons' | 'secrets' | 'logs' | 'services' | 'nutrition' | 'settings'

const navItems: { id: View; icon: typeof Brain; label: string }[] = [
  { id: 'home', icon: Home, label: 'Home' },
  { id: 'brain', icon: Brain, label: 'Brain' },
  { id: 'memory', icon: BookOpen, label: 'Memory' },
  { id: 'channels', icon: Radio, label: 'Channels' },
  { id: 'crons', icon: Clock, label: 'Crons' },
  { id: 'secrets', icon: KeyRound, label: 'Secrets' },
  { id: 'logs', icon: ScrollText, label: 'Logs' },
  { id: 'services', icon: HardDrive, label: 'Services' },
  { id: 'nutrition', icon: Flame, label: 'Nutrition' },
  { id: 'settings', icon: Settings, label: 'Settings' },
]

export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <nav className="fixed left-0 top-0 bottom-0 w-12 flex flex-col items-center pt-6 gap-2 z-50"
      style={{ background: 'rgba(0,0,0,0.4)', borderRight: '1px solid rgba(255,255,255,0.06)' }}>
      {navItems.map(({ id, icon: Icon, label }) => (
        <button
          key={id}
          onClick={() => onChange(id)}
          title={label}
          className={`w-9 h-9 flex items-center justify-center rounded-lg transition-all duration-200 ${
            active === id
              ? 'text-[#00d4ff]'
              : 'text-white/30 hover:text-white/60'
          }`}
          style={active === id ? {
            background: 'rgba(0, 212, 255, 0.08)',
            boxShadow: '0 0 12px rgba(0, 212, 255, 0.15)',
          } : undefined}
        >
          <Icon size={18} strokeWidth={active === id ? 2 : 1.5} />
        </button>
      ))}
    </nav>
  )
}

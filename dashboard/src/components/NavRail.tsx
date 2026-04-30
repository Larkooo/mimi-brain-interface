export type View = 'home' | 'brain' | 'memory' | 'channels' | 'crons' | 'secrets' | 'logs' | 'services' | 'nutrition' | 'tasks' | 'subagents' | 'settings'

const navItems: { id: View; label: string; cmd: string }[] = [
  { id: 'home',       label: 'overview',   cmd: 'o' },
  { id: 'tasks',      label: 'tasks',      cmd: 't' },
  { id: 'subagents',  label: 'subagents',  cmd: 'a' },
  { id: 'brain',      label: 'knowledge',  cmd: 'k' },
  { id: 'memory',     label: 'memory',     cmd: 'm' },
  { id: 'channels',   label: 'channels',   cmd: 'c' },
  { id: 'crons',      label: 'schedules',  cmd: 's' },
  { id: 'nutrition',  label: 'nutrition',  cmd: 'n' },
  { id: 'services',   label: 'services',   cmd: 'v' },
  { id: 'logs',       label: 'logs',       cmd: 'l' },
  { id: 'secrets',    label: 'secrets',    cmd: 'x' },
  { id: 'settings',   label: 'settings',   cmd: 'e' },
]

/**
 * Top terminal-style nav. Fixed, full-width, one row of lowercase text
 * with pipe separators — feels like a status bar at the top of a TUI.
 */
export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  return (
    <nav
      className="fixed top-0 left-0 right-0 z-50"
      style={{
        background: 'var(--background)',
        borderBottom: '1px solid var(--border)',
      }}
    >
      <div className="px-6 h-11 flex items-center gap-5 text-[12px]">
        <span className="flex items-center gap-2 pr-4 border-r border-border">
          <span style={{ color: 'var(--accentphosphor)' }}>▌</span>
          <span className="font-semibold tracking-wider uppercase">mimi</span>
          <span className="text-muted-foreground text-[10px]">v1</span>
        </span>
        {navItems.map(({ id, label }) => {
          const isActive = active === id
          return (
            <button
              key={id}
              onClick={() => onChange(id)}
              className="relative tracking-wide transition-colors py-1"
              style={{
                color: isActive ? 'var(--foreground)' : 'var(--muted-foreground)',
                borderBottom: isActive ? `2px solid var(--accentphosphor)` : '2px solid transparent',
                marginBottom: '-1px',
              }}
            >
              {isActive && <span style={{ color: 'var(--accentphosphor)', marginRight: 6 }}>&gt;</span>}
              {label}
            </button>
          )
        })}
      </div>
    </nav>
  )
}

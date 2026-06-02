import { useEffect } from 'react'

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
 *
 * Single-key shortcuts: pressing the bracketed letter on any view jumps
 * there (e.g. `t` → tasks). Suppressed while typing in an input/textarea
 * or when a modifier key is held, so it never fights with browser/native
 * shortcuts.
 */
export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const target = e.target as HTMLElement | null
      if (target) {
        const tag = target.tagName
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable) return
      }
      const key = e.key.toLowerCase()
      const match = navItems.find(n => n.cmd === key)
      if (!match) return
      e.preventDefault()
      onChange(match.id)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onChange])

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
        {navItems.map(({ id, label, cmd }) => {
          const isActive = active === id
          return (
            <button
              key={id}
              onClick={() => onChange(id)}
              title={`shortcut: ${cmd}`}
              className="relative tracking-wide transition-colors py-1"
              style={{
                color: isActive ? 'var(--foreground)' : 'var(--muted-foreground)',
                borderBottom: isActive ? `2px solid var(--accentphosphor)` : '2px solid transparent',
                marginBottom: '-1px',
              }}
            >
              {isActive && <span style={{ color: 'var(--accentphosphor)', marginRight: 6 }}>&gt;</span>}
              {label}
              <span
                className="ml-1.5 text-[10px] opacity-60"
                style={{ color: isActive ? 'var(--accentphosphor)' : undefined }}
              >
                [{cmd}]
              </span>
            </button>
          )
        })}
      </div>
    </nav>
  )
}

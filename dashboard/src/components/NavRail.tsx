import { useEffect } from 'react'

export type View = 'home' | 'brain' | 'memory' | 'channels' | 'crons' | 'secrets' | 'logs' | 'services' | 'nutrition' | 'tasks' | 'settings'

const navItems: { id: View; label: string; cmd: string }[] = [
  { id: 'home',      label: 'overview',   cmd: 'o' },
  { id: 'tasks',     label: 'tasks',      cmd: 't' },
  { id: 'brain',     label: 'knowledge',  cmd: 'k' },
  { id: 'memory',    label: 'memory',     cmd: 'm' },
  { id: 'channels',  label: 'channels',   cmd: 'c' },
  { id: 'crons',     label: 'schedules',  cmd: 's' },
  { id: 'nutrition', label: 'nutrition',  cmd: 'n' },
  { id: 'services',  label: 'services',   cmd: 'v' },
  { id: 'logs',      label: 'logs',       cmd: 'l' },
  { id: 'secrets',   label: 'secrets',    cmd: 'x' },
  { id: 'settings',  label: 'settings',   cmd: 'e' },
]

// True when the user is typing into a form field. Global shortcuts must
// stand down so we don't eat real keystrokes (e.g. typing "c" in a textarea
// shouldn't jump to channels).
function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false
  const tag = el.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  return el.isContentEditable
}

/**
 * Top terminal-style nav. Fixed, full-width, one row of lowercase text
 * with pipe separators — feels like a status bar at the top of a TUI.
 *
 * Each item exposes a single-letter shortcut (rendered as a faint hint
 * after the label). Pressing that key anywhere in the app jumps to the
 * corresponding view, unless the user is typing in a form field or holding
 * a modifier (so browser shortcuts like Cmd+R still work).
 */
export function NavRail({ active, onChange }: { active: View; onChange: (v: View) => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (isTypingTarget(e.target)) return
      const hit = navItems.find(n => n.cmd === e.key.toLowerCase())
      if (!hit) return
      e.preventDefault()
      onChange(hit.id)
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
              className="relative tracking-wide transition-colors py-1 group"
              style={{
                color: isActive ? 'var(--foreground)' : 'var(--muted-foreground)',
                borderBottom: isActive ? `2px solid var(--accentphosphor)` : '2px solid transparent',
                marginBottom: '-1px',
              }}
            >
              {isActive && <span style={{ color: 'var(--accentphosphor)', marginRight: 6 }}>&gt;</span>}
              {label}
              <span
                className="ml-1.5 text-[10px] opacity-40 group-hover:opacity-70 transition-opacity"
                style={{ color: 'var(--muted-foreground)' }}
              >
                {cmd}
              </span>
            </button>
          )
        })}
      </div>
    </nav>
  )
}

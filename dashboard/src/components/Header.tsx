import type { Status } from '../hooks/useApi'

export function Header({ status }: { status: Status | null }) {
  const name = status?.name ?? 'Mimi'

  return (
    <header style={{
      padding: '1.5rem 2rem',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      borderBottom: '1px solid var(--border)',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
        <div style={{
          width: 32,
          height: 32,
          borderRadius: '50%',
          background: status?.session_running
            ? 'radial-gradient(circle, var(--accent) 0%, transparent 70%)'
            : 'radial-gradient(circle, var(--danger) 0%, transparent 70%)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          boxShadow: status?.session_running
            ? '0 0 20px var(--accent-glow)'
            : 'none',
        }}>
          <div style={{
            width: 10,
            height: 10,
            borderRadius: '50%',
            background: status?.session_running ? 'var(--accent)' : 'var(--danger)',
          }} />
        </div>
        <span style={{
          fontSize: '1.1rem',
          fontWeight: 600,
          color: 'var(--text-bright)',
          letterSpacing: '-0.02em',
        }}>
          {name}
        </span>
        {status && (
          <span className={`tag ${status.session_running ? 'tag-success' : 'tag-danger'}`}>
            {status.session_running ? 'online' : 'offline'}
          </span>
        )}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: '1rem' }}>
        {status && (
          <span style={{ fontSize: '0.75rem', color: 'var(--text-dim)', fontFamily: "'JetBrains Mono', monospace" }}>
            {status.claude_version}
          </span>
        )}
      </div>
    </header>
  )
}

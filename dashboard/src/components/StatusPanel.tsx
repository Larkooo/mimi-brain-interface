import type { Status } from '../hooks/useApi'
import { launchSession, stopSession, createBackup } from '../hooks/useApi'

export function StatusPanel({ status, onRefresh }: { status: Status | null; onRefresh: () => void }) {
  if (!status) return <div className="empty">Connecting...</div>

  const bs = status.brain_stats

  return (
    <>
      <div className="grid-3">
        {/* Session card */}
        <div className="card">
          <h3>Session</h3>
          <div style={{ display: 'flex', gap: '1.5rem', marginBottom: '1rem' }}>
            <div>
              <div className="big-number" style={{ color: status.session_running ? 'var(--success)' : 'var(--danger)' }}>
                {status.session_running ? 'ON' : 'OFF'}
              </div>
              <div className="big-label">status</div>
            </div>
          </div>
          <div className="btn-row">
            <button className="btn btn-success" onClick={async () => { await launchSession(); setTimeout(onRefresh, 1000); }}>
              Launch
            </button>
            <button className="btn btn-danger" onClick={async () => { await stopSession(); setTimeout(onRefresh, 500); }}>
              Stop
            </button>
          </div>
        </div>

        {/* Brain card */}
        <div className="card">
          <h3>Knowledge Graph</h3>
          <div style={{ display: 'flex', gap: '1.5rem', marginBottom: '0.5rem' }}>
            <div>
              <div className="big-number" style={{ color: 'var(--accent)' }}>{bs.entities}</div>
              <div className="big-label">entities</div>
            </div>
            <div>
              <div className="big-number" style={{ color: 'var(--purple)' }}>{bs.relationships}</div>
              <div className="big-label">links</div>
            </div>
            <div>
              <div className="big-number" style={{ color: 'var(--text-muted)' }}>{bs.memory_refs}</div>
              <div className="big-label">mem refs</div>
            </div>
          </div>
          {bs.entity_types.length > 0 && (
            <div style={{ marginTop: '0.5rem' }}>
              {bs.entity_types.map(([type, count]) => (
                <div className="stat-row" key={type}>
                  <span className="stat-label">{type}</span>
                  <span className="stat-value">{count}</span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Memory card */}
        <div className="card">
          <h3>Memory</h3>
          <div style={{ marginBottom: '1rem' }}>
            <div className="big-number">{status.memory_files}</div>
            <div className="big-label">memory files</div>
          </div>
          <div style={{ marginBottom: '0.75rem' }}>
            <div className="stat-row">
              <span className="stat-label">Channels</span>
              <span className="stat-value">{status.channels.length}</span>
            </div>
            <div className="stat-row">
              <span className="stat-label">Active channels</span>
              <span className="stat-value">{status.channels.filter(c => c.enabled).length}</span>
            </div>
          </div>
          <button className="btn" onClick={async () => { await createBackup(); alert('Backup created'); }}>
            Backup
          </button>
        </div>
      </div>

      {/* Channels quick view */}
      {status.channels.length > 0 && (
        <div className="card">
          <h3>Active Channels</h3>
          <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
            {status.channels.map(ch => (
              <span key={ch.name} className={`tag ${ch.enabled ? 'tag-accent' : 'tag-danger'}`}>
                {ch.name}
              </span>
            ))}
          </div>
        </div>
      )}
    </>
  )
}

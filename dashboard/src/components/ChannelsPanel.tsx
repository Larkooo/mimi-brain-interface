import { useState } from 'react'
import type { Channel } from '../hooks/useApi'
import { addChannel, removeChannel, toggleChannel } from '../hooks/useApi'

export function ChannelsPanel({ channels, onRefresh }: { channels: Channel[]; onRefresh: () => void }) {
  const [newType, setNewType] = useState('telegram')

  const handleAdd = async () => {
    await addChannel(newType)
    onRefresh()
  }

  return (
    <>
      <div className="card mb-1">
        <h3>Configured Channels</h3>
        {channels.length === 0 ? (
          <div className="empty">No channels configured. Add one below.</div>
        ) : (
          <div>
            {channels.map(ch => (
              <div key={ch.name} style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                padding: '0.6rem 0',
                borderBottom: '1px solid rgba(255,255,255,0.03)',
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
                  <span className={`dot ${ch.enabled ? 'on' : 'off'}`} />
                  <span style={{ color: 'var(--text-bright)', fontWeight: 500 }}>{ch.name}</span>
                  <span className="tag tag-purple">{ch.type}</span>
                  {ch.plugin && (
                    <span style={{ fontSize: '0.75rem', color: 'var(--text-dim)', fontFamily: "'JetBrains Mono', monospace" }}>
                      {ch.plugin}
                    </span>
                  )}
                </div>
                <div className="btn-row">
                  <button
                    className={`btn ${ch.enabled ? 'btn-danger' : 'btn-success'}`}
                    onClick={async () => { await toggleChannel(ch.name); onRefresh(); }}
                  >
                    {ch.enabled ? 'Disable' : 'Enable'}
                  </button>
                  <button
                    className="btn btn-danger"
                    onClick={async () => { await removeChannel(ch.name); onRefresh(); }}
                  >
                    Remove
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="card">
        <h3>Add Channel</h3>
        <div className="btn-row">
          <select className="input" style={{ width: 'auto' }} value={newType} onChange={e => setNewType(e.target.value)}>
            <option value="telegram">Telegram</option>
            <option value="discord">Discord</option>
            <option value="imessage">iMessage</option>
          </select>
          <button className="btn btn-accent" onClick={handleAdd}>Add Channel</button>
        </div>
        <div style={{ marginTop: '0.75rem', fontSize: '0.8rem', color: 'var(--text-dim)' }}>
          Adding a channel installs the Claude Code plugin and creates a config file.
          After adding, configure the bot token and relaunch Mimi.
        </div>
      </div>
    </>
  )
}

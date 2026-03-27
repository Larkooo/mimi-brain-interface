import { useState, useEffect } from 'react'
import { getConfig, saveConfig } from '../hooks/useApi'

export function ConfigPanel() {
  const [json, setJson] = useState('')
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getConfig().then(c => setJson(JSON.stringify(c, null, 2))).catch(() => {})
  }, [])

  const handleSave = async () => {
    try {
      const parsed = JSON.parse(json)
      await saveConfig(parsed)
      setSaved(true)
      setError(null)
      setTimeout(() => setSaved(false), 2000)
    } catch {
      setError('Invalid JSON')
    }
  }

  return (
    <div className="card">
      <h3>Configuration</h3>
      <textarea
        className="textarea mb-1"
        style={{ minHeight: 200 }}
        value={json}
        onChange={e => { setJson(e.target.value); setSaved(false); setError(null); }}
      />
      <div className="btn-row">
        <button className="btn btn-accent" onClick={handleSave}>Save Config</button>
        {saved && <span style={{ color: 'var(--success)', fontSize: '0.8rem' }}>Saved</span>}
        {error && <span style={{ color: 'var(--danger)', fontSize: '0.8rem' }}>{error}</span>}
      </div>
      <div style={{ marginTop: '0.75rem', fontSize: '0.8rem', color: 'var(--text-dim)' }}>
        Changes to session_name or model require a relaunch to take effect.
      </div>
    </div>
  )
}

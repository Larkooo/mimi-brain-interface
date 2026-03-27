import { useState } from 'react'
import { runQuery } from '../hooks/useApi'

export function QueryPanel() {
  const [sql, setSql] = useState('SELECT * FROM entities LIMIT 20')
  const [results, setResults] = useState<[string, string][][] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const execute = async () => {
    try {
      setError(null)
      const rows = await runQuery(sql)
      setResults(rows)
    } catch (e) {
      setError((e as Error).message)
      setResults(null)
    }
  }

  return (
    <div className="card">
      <h3>SQL Query</h3>
      <textarea
        className="textarea mb-1"
        value={sql}
        onChange={e => setSql(e.target.value)}
        onKeyDown={e => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') execute() }}
        placeholder="SELECT * FROM entities LIMIT 20"
        rows={4}
      />
      <div className="btn-row mb-1">
        <button className="btn btn-accent" onClick={execute}>
          Run Query
        </button>
        <span style={{ fontSize: '0.7rem', color: 'var(--text-dim)' }}>Cmd+Enter</span>
      </div>

      {error && <div style={{ color: 'var(--danger)', fontSize: '0.85rem', padding: '0.5rem 0' }}>{error}</div>}

      {results !== null && (
        results.length === 0 ? (
          <div className="empty">(no results)</div>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  {results[0].map(([col]) => <th key={col}>{col}</th>)}
                </tr>
              </thead>
              <tbody>
                {results.map((row, i) => (
                  <tr key={i}>
                    {row.map(([col, val]) => <td key={col}>{val}</td>)}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )
      )}

      <div style={{ marginTop: '1rem', fontSize: '0.75rem', color: 'var(--text-dim)' }}>
        <div style={{ marginBottom: '0.5rem', fontWeight: 600 }}>Quick queries:</div>
        {[
          'SELECT * FROM entities ORDER BY updated_at DESC LIMIT 20',
          'SELECT * FROM relationships r JOIN entities s ON r.source_id=s.id JOIN entities t ON r.target_id=t.id',
          'SELECT type, COUNT(*) as count FROM entities GROUP BY type',
          'SELECT type, COUNT(*) as count FROM relationships GROUP BY type',
        ].map(q => (
          <div
            key={q}
            style={{ cursor: 'pointer', padding: '0.25rem 0', fontFamily: "'JetBrains Mono', monospace" }}
            onClick={() => { setSql(q); }}
          >
            {q}
          </div>
        ))}
      </div>
    </div>
  )
}

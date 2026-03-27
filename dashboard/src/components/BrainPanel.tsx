import { useState, useEffect } from 'react'
import type { BrainStats, Entity } from '../hooks/useApi'
import { getEntities, searchEntities, addEntity, addRelationship } from '../hooks/useApi'

export function BrainPanel({ stats }: { stats?: BrainStats }) {
  const [entities, setEntities] = useState<Entity[]>([])
  const [search, setSearch] = useState('')
  const [newType, setNewType] = useState('person')
  const [newName, setNewName] = useState('')
  const [newProps, setNewProps] = useState('{}')
  const [linkSource, setLinkSource] = useState('')
  const [linkRel, setLinkRel] = useState('knows')
  const [linkTarget, setLinkTarget] = useState('')

  const load = async () => {
    if (search) {
      setEntities(await searchEntities(search))
    } else {
      setEntities(await getEntities())
    }
  }

  useEffect(() => {
    const timeout = setTimeout(load, search ? 300 : 0)
    return () => clearTimeout(timeout)
  }, [search])

  const handleAddEntity = async () => {
    if (!newName.trim()) return
    await addEntity(newType, newName, newProps)
    setNewName('')
    load()
  }

  const handleAddLink = async () => {
    const src = parseInt(linkSource)
    const tgt = parseInt(linkTarget)
    if (isNaN(src) || isNaN(tgt) || !linkRel) return
    await addRelationship(src, linkRel, tgt)
    setLinkSource('')
    setLinkTarget('')
    load()
  }

  return (
    <>
      {/* Stats overview */}
      {stats && (
        <div className="grid-3 mb-1">
          <div className="card" style={{ textAlign: 'center' }}>
            <div className="big-number" style={{ color: 'var(--accent)' }}>{stats.entities}</div>
            <div className="big-label">entities</div>
          </div>
          <div className="card" style={{ textAlign: 'center' }}>
            <div className="big-number" style={{ color: 'var(--purple)' }}>{stats.relationships}</div>
            <div className="big-label">relationships</div>
          </div>
          <div className="card" style={{ textAlign: 'center' }}>
            <div className="big-number" style={{ color: 'var(--success)' }}>{stats.memory_refs}</div>
            <div className="big-label">memory refs</div>
          </div>
        </div>
      )}

      {/* Search + Entity list */}
      <div className="card mb-1">
        <h3>Entities</h3>
        <input
          className="input mb-1"
          placeholder="Search entities..."
          value={search}
          onChange={e => setSearch(e.target.value)}
        />
        {entities.length === 0 ? (
          <div className="empty">{search ? 'No results' : 'No entities yet'}</div>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr><th>ID</th><th>Type</th><th>Name</th><th>Properties</th><th>Created</th></tr>
              </thead>
              <tbody>
                {entities.map(e => (
                  <tr key={e.id}>
                    <td style={{ color: 'var(--accent)' }}>{e.id}</td>
                    <td><span className="tag tag-purple">{e.type}</span></td>
                    <td style={{ color: 'var(--text-bright)' }}>{e.name}</td>
                    <td style={{ color: 'var(--text-muted)', maxWidth: 200, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                      {JSON.stringify(e.properties) !== '{}' ? JSON.stringify(e.properties) : ''}
                    </td>
                    <td style={{ color: 'var(--text-dim)' }}>{e.created_at}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Add entity */}
      <div className="grid-2">
        <div className="card">
          <h3>Add Entity</h3>
          <div className="btn-row mb-1">
            <select className="input" style={{ width: 'auto' }} value={newType} onChange={e => setNewType(e.target.value)}>
              {['person', 'company', 'service', 'concept', 'account', 'project', 'location', 'event'].map(t => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
            <input className="input" placeholder="Name" value={newName} onChange={e => setNewName(e.target.value)} style={{ flex: 1 }} />
          </div>
          <input className="input mb-1" placeholder='Properties JSON: {"key":"value"}' value={newProps} onChange={e => setNewProps(e.target.value)} />
          <button className="btn btn-accent" onClick={handleAddEntity}>Add Entity</button>
        </div>

        {/* Add relationship */}
        <div className="card">
          <h3>Add Relationship</h3>
          <div className="btn-row mb-1">
            <input className="input" placeholder="Source ID" value={linkSource} onChange={e => setLinkSource(e.target.value)} style={{ width: 80 }} />
            <select className="input" style={{ width: 'auto' }} value={linkRel} onChange={e => setLinkRel(e.target.value)}>
              {['knows', 'works_at', 'has_account', 'owns', 'related_to', 'created', 'member_of', 'depends_on'].map(t => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
            <input className="input" placeholder="Target ID" value={linkTarget} onChange={e => setLinkTarget(e.target.value)} style={{ width: 80 }} />
          </div>
          <button className="btn btn-accent" onClick={handleAddLink}>Link</button>
        </div>
      </div>
    </>
  )
}

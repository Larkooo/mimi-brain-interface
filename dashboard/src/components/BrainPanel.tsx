import { useState, useEffect } from 'react'
import type { BrainStats, Entity } from '../hooks/useApi'
import { getEntities, searchEntities, addEntity, addRelationship } from '../hooks/useApi'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

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

  const types = ['person', 'company', 'service', 'concept', 'account', 'project', 'location', 'event']
  const relTypes = ['knows', 'works_at', 'has_account', 'owns', 'related_to', 'created', 'member_of', 'depends_on']

  return (
    <div className="space-y-4">
      {stats && (
        <div className="grid grid-cols-3 gap-4">
          {[
            { label: 'Entities', value: stats.entities, color: 'text-blue-400' },
            { label: 'Relationships', value: stats.relationships, color: 'text-purple-400' },
            { label: 'Memory Refs', value: stats.memory_refs, color: 'text-emerald-400' },
          ].map(s => (
            <Card key={s.label}>
              <CardContent className="pt-6 text-center">
                <div className={`text-3xl font-bold font-mono ${s.color}`}>{s.value}</div>
                <div className="text-xs text-muted-foreground mt-1">{s.label}</div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Entities</CardTitle>
        </CardHeader>
        <CardContent>
          <Input
            placeholder="Search entities..."
            value={search}
            onChange={e => setSearch(e.target.value)}
            className="mb-4"
          />
          {entities.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-4">
              {search ? 'No results' : 'No entities yet'}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-16">ID</TableHead>
                  <TableHead>Type</TableHead>
                  <TableHead>Name</TableHead>
                  <TableHead>Properties</TableHead>
                  <TableHead>Created</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {entities.map(e => (
                  <TableRow key={e.id}>
                    <TableCell className="font-mono text-blue-400">{e.id}</TableCell>
                    <TableCell><Badge variant="secondary">{e.type}</Badge></TableCell>
                    <TableCell className="font-medium">{e.name}</TableCell>
                    <TableCell className="text-muted-foreground text-xs font-mono max-w-[200px] truncate">
                      {JSON.stringify(e.properties) !== '{}' ? JSON.stringify(e.properties) : ''}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">{e.created_at}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Add Entity</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="flex gap-2">
              <select className="flex h-9 w-auto rounded-md border border-input bg-transparent px-3 py-1 text-sm" value={newType} onChange={e => setNewType(e.target.value)}>
                {types.map(t => <option key={t} value={t}>{t}</option>)}
              </select>
              <Input placeholder="Name" value={newName} onChange={e => setNewName(e.target.value)} />
            </div>
            <Input placeholder='Properties: {"key":"value"}' value={newProps} onChange={e => setNewProps(e.target.value)} />
            <Button size="sm" onClick={handleAddEntity}>Add Entity</Button>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Add Relationship</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <div className="flex gap-2">
              <Input placeholder="Source ID" value={linkSource} onChange={e => setLinkSource(e.target.value)} className="w-24" />
              <select className="flex h-9 w-auto rounded-md border border-input bg-transparent px-3 py-1 text-sm" value={linkRel} onChange={e => setLinkRel(e.target.value)}>
                {relTypes.map(t => <option key={t} value={t}>{t}</option>)}
              </select>
              <Input placeholder="Target ID" value={linkTarget} onChange={e => setLinkTarget(e.target.value)} className="w-24" />
            </div>
            <Button size="sm" onClick={handleAddLink}>Link</Button>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}

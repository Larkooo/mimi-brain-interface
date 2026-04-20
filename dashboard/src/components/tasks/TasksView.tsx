import { useCallback, useEffect, useMemo, useState } from 'react'
import type { Task, TaskTreeNode, TaskDetail } from '../../hooks/useApi'
import { getTaskTree, getTask, createTask, updateTask, deleteTask } from '../../hooks/useApi'

type Status = Task['status']

const STATUS_ICON: Record<Status, string> = {
  running: '▶', pending: '○', blocked: '⊘', done: '✓', cancelled: '—', failed: '✗',
}

const STATUS_COLOR: Record<Status, string> = {
  running: 'var(--accentphosphor)',
  pending: 'var(--muted-foreground)',
  blocked: 'var(--warning)',
  done: 'var(--accentphosphor)',
  cancelled: 'var(--muted-foreground)',
  failed: 'var(--danger)',
}

export function TasksView() {
  const [tree, setTree] = useState<TaskTreeNode[]>([])
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [detail, setDetail] = useState<TaskDetail | null>(null)
  const [filter, setFilter] = useState<Status | 'all'>('all')
  const [creating, setCreating] = useState(false)

  const refreshTree = useCallback(async () => {
    try { setTree(await getTaskTree()) } catch {}
  }, [])

  useEffect(() => {
    refreshTree()
    const t = setInterval(refreshTree, 3000)
    return () => clearInterval(t)
  }, [refreshTree])

  useEffect(() => {
    if (selectedId == null) { setDetail(null); return }
    let cancelled = false
    const load = async () => {
      try {
        const d = await getTask(selectedId)
        if (!cancelled) setDetail(d)
      } catch { if (!cancelled) setDetail(null) }
    }
    load()
    const t = setInterval(load, 3000)
    return () => { cancelled = true; clearInterval(t) }
  }, [selectedId])

  const filtered = useMemo(() => {
    if (filter === 'all') return tree
    return tree.filter(n => n.status === filter)
  }, [tree, filter])

  const counts = useMemo(() => {
    const c: Record<Status, number> = { pending: 0, running: 0, blocked: 0, done: 0, cancelled: 0, failed: 0 }
    tree.forEach(n => { c[n.status] = (c[n.status] ?? 0) + 1 })
    return c
  }, [tree])

  return (
    <div className="px-8 pt-10 pb-20 max-w-7xl mx-auto">
      <header className="mb-8">
        <div className="flex items-baseline gap-2 mb-1">
          <span style={{ color: 'var(--accentphosphor)' }}>&gt;</span>
          <h1 className="text-[20px] font-semibold tracking-wide lowercase">tasks</h1>
        </div>
        <p className="text-[13px] text-muted-foreground ml-4">
          centralized task tracking across channels. infinite parent/child depth.
        </p>
      </header>

      {/* Summary counts */}
      <section className="flex items-center gap-1 mb-6">
        <FilterChip active={filter === 'all'} onClick={() => setFilter('all')}>
          all · {tree.length}
        </FilterChip>
        {(['running','pending','blocked','done','failed','cancelled'] as Status[]).map(s => (
          <FilterChip key={s} active={filter === s} onClick={() => setFilter(s)} color={STATUS_COLOR[s]}>
            {s} · {counts[s] ?? 0}
          </FilterChip>
        ))}
        <div className="flex-1" />
        <button
          onClick={() => setCreating(v => !v)}
          className="term-btn"
        >
          + new task
        </button>
      </section>

      {creating && (
        <CreateForm
          onCreated={async () => { setCreating(false); await refreshTree() }}
          onCancel={() => setCreating(false)}
        />
      )}

      <section className="grid grid-cols-12 gap-4 mt-4">
        {/* Tree — left column */}
        <div className="col-span-12 md:col-span-5 term-box">
          <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
            <span style={{ color: 'var(--accentphosphor)' }}>┌─</span>
            <h3 className="text-[12px] font-semibold uppercase tracking-[0.18em]">tree</h3>
            <span className="flex-1 border-t border-border/70 ml-2" />
            <span className="text-[11px] text-muted-foreground num">{filtered.length}</span>
          </div>
          <div className="max-h-[70vh] overflow-y-auto">
            {filtered.length === 0 && (
              <div className="px-4 py-6 text-[12px] text-muted-foreground">
                # no tasks. use `task add` or click "+ new task" above.
              </div>
            )}
            {filtered.map(n => (
              <TreeRow
                key={n.id}
                node={n}
                selected={selectedId === n.id}
                onClick={() => setSelectedId(n.id)}
              />
            ))}
          </div>
        </div>

        {/* Detail — right column */}
        <div className="col-span-12 md:col-span-7 term-box">
          {detail ? (
            <Detail
              detail={detail}
              onStatusChange={async (status, note) => {
                await updateTask(detail.id, { status, note, author: 'web' })
                const d = await getTask(detail.id)
                setDetail(d)
                await refreshTree()
              }}
              onDelete={async () => {
                await deleteTask(detail.id)
                setSelectedId(null)
                await refreshTree()
              }}
            />
          ) : (
            <div className="px-6 py-10 text-[12px] text-muted-foreground">
              # select a task on the left to see its updates and children.
            </div>
          )}
        </div>
      </section>
    </div>
  )
}

function FilterChip({ active, onClick, color, children }: { active: boolean; onClick: () => void; color?: string; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className="px-2.5 py-1 text-[11px] uppercase tracking-wider transition-colors"
      style={{
        color: active ? (color ?? 'var(--foreground)') : 'var(--muted-foreground)',
        border: `1px solid ${active ? (color ?? 'var(--foreground)') : 'var(--border)'}`,
        background: active ? 'var(--muted)' : 'transparent',
      }}
    >
      {children}
    </button>
  )
}

function TreeRow({ node, selected, onClick }: { node: TaskTreeNode; selected: boolean; onClick: () => void }) {
  const indent = 8 + node.depth * 14
  return (
    <button
      onClick={onClick}
      className="w-full text-left px-3 py-1.5 text-[12px] transition-colors"
      style={{
        background: selected ? 'var(--muted)' : 'transparent',
        borderLeft: selected ? `2px solid var(--accentphosphor)` : '2px solid transparent',
      }}
    >
      <div className="flex items-center gap-2" style={{ paddingLeft: indent }}>
        <span style={{ color: STATUS_COLOR[node.status] }}>{STATUS_ICON[node.status]}</span>
        <span className="text-muted-foreground text-[10px] num">#{node.id}</span>
        <span className="truncate">{node.title}</span>
        {node.progress > 0 && node.status === 'running' && (
          <span className="text-[10px] text-muted-foreground num ml-auto">{node.progress}%</span>
        )}
      </div>
    </button>
  )
}

function Detail({ detail, onStatusChange, onDelete }: {
  detail: TaskDetail
  onStatusChange: (status: Status, note?: string) => Promise<void>
  onDelete: () => Promise<void>
}) {
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  const transition = async (status: Status) => {
    setBusy(true)
    try {
      await onStatusChange(status, note.trim() || undefined)
      setNote('')
    } finally { setBusy(false) }
  }

  return (
    <div>
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border">
        <span style={{ color: STATUS_COLOR[detail.status] }}>{STATUS_ICON[detail.status]}</span>
        <span className="text-muted-foreground text-[10px] num">#{detail.id}</span>
        <h3 className="text-[13px] font-medium truncate flex-1">{detail.title}</h3>
        <button
          onClick={onDelete}
          className="text-[10px] text-muted-foreground hover:text-danger uppercase tracking-wider"
        >
          delete
        </button>
      </div>

      <div className="px-4 py-3 grid grid-cols-2 gap-x-6 gap-y-1.5 text-[12px]">
        <KV k="status">
          <span className="term-tag" style={{ color: STATUS_COLOR[detail.status] }}>
            {detail.status}
          </span>
        </KV>
        <KV k="progress">{detail.progress}%</KV>
        <KV k="origin">{detail.origin_channel ?? '-'} / {detail.origin_user ?? '-'}</KV>
        <KV k="assignee">{detail.assignee ?? '-'}</KV>
        <KV k="depth">{detail.depth}</KV>
        <KV k="parent">{detail.parent_id ?? '-'}</KV>
        <KV k="created">{detail.created_at}</KV>
        <KV k="updated">{detail.updated_at}</KV>
        {detail.started_at && <KV k="started">{detail.started_at}</KV>}
        {detail.completed_at && <KV k="completed">{detail.completed_at}</KV>}
      </div>

      {detail.description && (
        <div className="px-4 py-3 border-t border-border">
          <div className="eyebrow mb-1.5">description</div>
          <p className="text-[12px] whitespace-pre-wrap">{detail.description}</p>
        </div>
      )}

      {detail.children.length > 0 && (
        <div className="px-4 py-3 border-t border-border">
          <div className="eyebrow mb-2">children ({detail.children.length})</div>
          <div className="space-y-0.5">
            {detail.children.map(c => (
              <div key={c.id} className="flex items-center gap-2 text-[12px]">
                <span style={{ color: STATUS_COLOR[c.status] }}>{STATUS_ICON[c.status]}</span>
                <span className="text-muted-foreground text-[10px] num">#{c.id}</span>
                <span className="truncate">{c.title}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="px-4 py-3 border-t border-border">
        <div className="eyebrow mb-2">updates</div>
        <div className="space-y-1.5">
          {detail.updates.map(u => (
            <div key={u.id} className="text-[11px] font-mono">
              <span className="text-muted-foreground">[{u.created_at}]</span>{' '}
              <span>{u.author ?? '?'}</span>
              {u.status_before && u.status_after && u.status_before !== u.status_after && (
                <>
                  {' '}<span style={{ color: 'var(--muted-foreground)' }}>{u.status_before}</span>
                  {' → '}
                  <span style={{ color: STATUS_COLOR[u.status_after as Status] }}>{u.status_after}</span>
                </>
              )}
              {u.note && <>{'  '}<span>— {u.note}</span></>}
            </div>
          ))}
        </div>
      </div>

      {/* Transition controls */}
      <div className="px-4 py-3 border-t border-border">
        <div className="eyebrow mb-2">transition</div>
        <input
          placeholder="optional note..."
          value={note}
          onChange={e => setNote(e.target.value)}
          className="w-full mb-2 px-2 py-1.5 bg-muted border border-border text-[12px] font-mono"
        />
        <div className="flex gap-1 flex-wrap">
          <button className="term-btn" disabled={busy} onClick={() => transition('running')}>▶ start</button>
          <button className="term-btn" disabled={busy} onClick={() => transition('blocked')}>⊘ block</button>
          <button className="term-btn" disabled={busy} onClick={() => transition('done')}>✓ done</button>
          <button className="term-btn" disabled={busy} onClick={() => transition('failed')}>✗ fail</button>
          <button className="term-btn" disabled={busy} onClick={() => transition('cancelled')}>— cancel</button>
        </div>
      </div>
    </div>
  )
}

function KV({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-muted-foreground text-[11px] lowercase">{k}</span>
      <span className="flex-1 border-b border-dotted border-border mb-1" />
      <span className="num">{children}</span>
    </div>
  )
}

function CreateForm({ onCreated, onCancel }: { onCreated: () => void; onCancel: () => void }) {
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    if (!title.trim()) return
    setBusy(true)
    try {
      await createTask({ title: title.trim(), description: description.trim() || undefined, origin_channel: 'web', origin_user: 'web' })
      setTitle(''); setDescription('')
      onCreated()
    } finally { setBusy(false) }
  }
  return (
    <div className="term-box p-4 space-y-3">
      <div className="flex items-center gap-2">
        <span style={{ color: 'var(--accentphosphor)' }}>$</span>
        <input
          placeholder="task title..."
          value={title}
          autoFocus
          onChange={e => setTitle(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter' && e.metaKey) submit() }}
          className="flex-1 bg-transparent text-[13px] font-mono outline-none"
        />
      </div>
      <textarea
        placeholder="# optional description..."
        value={description}
        onChange={e => setDescription(e.target.value)}
        className="w-full px-2 py-1.5 bg-muted border border-border text-[12px] font-mono min-h-[60px]"
      />
      <div className="flex gap-2">
        <button className="term-btn" onClick={submit} disabled={busy || !title.trim()}>create</button>
        <button className="term-btn" onClick={onCancel}>cancel</button>
      </div>
    </div>
  )
}

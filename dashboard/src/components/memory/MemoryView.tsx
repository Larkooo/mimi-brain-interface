import { useState, useEffect } from 'react'
import { useMemoryFiles, getMemoryFile } from '../../hooks/useApi'
import { BookOpen, FileText, User, MessageSquare, FolderOpen, Bookmark, Search } from 'lucide-react'
import { Input } from '@/components/ui/input'

const TYPE_CONFIG: Record<string, { icon: typeof FileText; label: string }> = {
  user:      { icon: User,         label: 'User' },
  feedback:  { icon: MessageSquare,label: 'Feedback' },
  project:   { icon: FolderOpen,   label: 'Project' },
  reference: { icon: Bookmark,     label: 'Reference' },
}

function TypeBadge({ type }: { type: string }) {
  const config = TYPE_CONFIG[type] || { icon: FileText, label: type || 'unknown' }
  const Icon = config.icon
  return (
    <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded border border-border bg-muted/40 text-[10px] uppercase tracking-wider text-muted-foreground">
      <Icon size={10} />
      {config.label}
    </span>
  )
}

export function MemoryView() {
  const { files } = useMemoryFiles()
  const [selected, setSelected] = useState<string | null>(null)
  const [content, setContent] = useState<string>('')
  const [loading, setLoading] = useState(false)
  const [filter, setFilter] = useState('')
  const [typeFilter, setTypeFilter] = useState<string | null>(null)

  useEffect(() => {
    if (!selected) {
      setContent('')
      return
    }
    setLoading(true)
    getMemoryFile(selected)
      .then(res => setContent(res.content))
      .catch(() => setContent('Failed to load file'))
      .finally(() => setLoading(false))
  }, [selected])

  const types = [...new Set(files.map(f => f.type).filter(Boolean))]

  const filtered = files.filter(f => {
    if (typeFilter && f.type !== typeFilter) return false
    if (filter) {
      const q = filter.toLowerCase()
      return f.name.toLowerCase().includes(q) || f.description.toLowerCase().includes(q) || f.filename.toLowerCase().includes(q)
    }
    return true
  })

  const grouped: Record<string, typeof filtered> = {}
  for (const f of filtered) {
    const key = f.type || 'other'
    if (!grouped[key]) grouped[key] = []
    grouped[key].push(f)
  }

  return (
    <div className="h-screen flex">
      <aside className="w-[300px] shrink-0 h-full flex flex-col border-r border-border">
        <div className="px-4 pt-6 pb-3 space-y-3">
          <div className="flex items-baseline justify-between">
            <div>
              <div className="eyebrow">Section</div>
              <h1 className="text-[20px] font-semibold tracking-tight leading-none">Memory</h1>
            </div>
            <span className="text-[11px] text-muted-foreground num">{files.length}</span>
          </div>

          <div className="relative">
            <Search size={13} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground/60" />
            <Input
              placeholder="Search memories..."
              value={filter}
              onChange={e => setFilter(e.target.value)}
              className="bg-muted/40 border-border text-xs pl-8 h-8"
            />
          </div>

          <div className="flex gap-1 flex-wrap">
            <FilterChip active={!typeFilter} onClick={() => setTypeFilter(null)}>All</FilterChip>
            {types.map(t => (
              <FilterChip key={t} active={typeFilter === t} onClick={() => setTypeFilter(typeFilter === t ? null : t)}>
                {t}
              </FilterChip>
            ))}
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-2 pb-4">
          {Object.entries(grouped).map(([type, items]) => (
            <div key={type} className="mb-3">
              <div className="px-2 py-1.5 eyebrow">
                {TYPE_CONFIG[type]?.label || type} <span className="text-muted-foreground/60 num">({items.length})</span>
              </div>
              {items.map(f => (
                <button
                  key={f.filename}
                  onClick={() => setSelected(f.filename)}
                  className={[
                    'w-full text-left px-3 py-2 rounded-md mb-0.5 transition-colors',
                    selected === f.filename
                      ? 'bg-accent text-foreground'
                      : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
                  ].join(' ')}
                >
                  <div className="text-[12px] truncate font-medium tracking-tight">
                    {f.name || f.filename}
                  </div>
                  {f.description && (
                    <p className="text-[10px] text-muted-foreground/70 truncate mt-0.5">{f.description}</p>
                  )}
                </button>
              ))}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="text-center py-10">
              <BookOpen size={22} strokeWidth={1.4} className="mx-auto mb-2 text-muted-foreground/40" />
              <p className="text-[12px] text-muted-foreground">No memories found</p>
            </div>
          )}
        </div>
      </aside>

      <section className="flex-1 h-full overflow-y-auto">
        {selected ? (
          <div className="px-10 py-10 max-w-3xl">
            <div className="flex items-center gap-3 mb-5">
              <FileText size={15} className="text-muted-foreground" />
              <h2 className="text-[15px] font-medium tracking-tight">{selected}</h2>
              {files.find(f => f.filename === selected)?.type && (
                <TypeBadge type={files.find(f => f.filename === selected)!.type} />
              )}
            </div>
            {loading ? (
              <div className="text-[12px] text-muted-foreground">Loading...</div>
            ) : (
              <pre className="text-[12px] text-foreground/80 whitespace-pre-wrap font-mono leading-relaxed surface p-5">
                {content}
              </pre>
            )}
          </div>
        ) : (
          <div className="h-full flex items-center justify-center">
            <div className="text-center">
              <BookOpen size={28} strokeWidth={1.4} className="mx-auto mb-3 text-muted-foreground/40" />
              <p className="text-[13px] text-muted-foreground">Select a memory to view</p>
              <p className="text-[11px] text-muted-foreground/60 mt-1">Mimi's persistent recollections.</p>
            </div>
          </div>
        )}
      </section>
    </div>
  )
}

function FilterChip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={[
        'px-2 py-0.5 rounded-md text-[10px] uppercase tracking-wider border transition-colors',
        active
          ? 'border-border bg-accent text-foreground'
          : 'border-border/60 bg-transparent text-muted-foreground hover:text-foreground hover:bg-accent/60',
      ].join(' ')}
    >
      {children}
    </button>
  )
}

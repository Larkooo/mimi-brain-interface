import { useState, useEffect } from 'react'
import { useMemoryFiles, getMemoryFile } from '../../hooks/useApi'
import { BookOpen, FileText, User, MessageSquare, FolderOpen, Bookmark, Search } from 'lucide-react'
import { Input } from '@/components/ui/input'

const TYPE_CONFIG: Record<string, { icon: typeof FileText; color: string; label: string }> = {
  user: { icon: User, color: '#00d4ff', label: 'User' },
  feedback: { icon: MessageSquare, color: '#00ffa3', label: 'Feedback' },
  project: { icon: FolderOpen, color: '#863bff', label: 'Project' },
  reference: { icon: Bookmark, color: '#ff9f43', label: 'Reference' },
}

function TypeBadge({ type }: { type: string }) {
  const config = TYPE_CONFIG[type] || { icon: FileText, color: '#888', label: type || 'unknown' }
  const Icon = config.icon
  return (
    <span
      className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium uppercase tracking-wider"
      style={{ color: config.color, background: `${config.color}15`, border: `1px solid ${config.color}25` }}
    >
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
    <div className="h-full flex">
      {/* Left panel - file list */}
      <div className="w-80 shrink-0 h-full flex flex-col border-r border-white/6">
        <div className="p-4 pb-3 space-y-3">
          <div className="flex items-center gap-2">
            <BookOpen size={18} className="text-[#00d4ff]" />
            <h1 className="text-sm font-medium text-white/90">Memory</h1>
            <span className="text-[10px] text-white/25 ml-auto">{files.length} files</span>
          </div>

          <div className="relative">
            <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-white/20" />
            <Input
              placeholder="Search memories..."
              value={filter}
              onChange={e => setFilter(e.target.value)}
              className="bg-white/5 border-white/10 text-white/80 text-xs pl-8 h-8"
            />
          </div>

          <div className="flex gap-1 flex-wrap">
            <button
              onClick={() => setTypeFilter(null)}
              className={`px-2 py-0.5 rounded text-[10px] transition-all ${
                !typeFilter
                  ? 'bg-[#00d4ff]/15 text-[#00d4ff] border border-[#00d4ff]/30'
                  : 'bg-white/5 text-white/30 border border-white/8 hover:text-white/50'
              }`}
            >
              All
            </button>
            {types.map(t => {
              const config = TYPE_CONFIG[t]
              return (
                <button
                  key={t}
                  onClick={() => setTypeFilter(typeFilter === t ? null : t)}
                  className={`px-2 py-0.5 rounded text-[10px] transition-all ${
                    typeFilter === t
                      ? `border`
                      : 'bg-white/5 text-white/30 border border-white/8 hover:text-white/50'
                  }`}
                  style={typeFilter === t ? {
                    color: config?.color || '#888',
                    background: `${config?.color || '#888'}15`,
                    borderColor: `${config?.color || '#888'}30`,
                  } : undefined}
                >
                  {t}
                </button>
              )
            })}
          </div>
        </div>

        <div className="flex-1 overflow-y-auto px-2 pb-4">
          {Object.entries(grouped).map(([type, items]) => (
            <div key={type} className="mb-3">
              <div className="px-2 py-1.5 text-[10px] text-white/20 uppercase tracking-wider font-medium">
                {TYPE_CONFIG[type]?.label || type} ({items.length})
              </div>
              {items.map(f => (
                <button
                  key={f.filename}
                  onClick={() => setSelected(f.filename)}
                  className={`w-full text-left px-3 py-2 rounded-lg mb-0.5 transition-all ${
                    selected === f.filename
                      ? 'bg-[#00d4ff]/10 border border-[#00d4ff]/20'
                      : 'hover:bg-white/5 border border-transparent'
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span className={`text-xs truncate ${selected === f.filename ? 'text-[#00d4ff]' : 'text-white/70'}`}>
                      {f.name || f.filename}
                    </span>
                  </div>
                  {f.description && (
                    <p className="text-[10px] text-white/25 truncate mt-0.5">{f.description}</p>
                  )}
                </button>
              ))}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="text-center py-10">
              <BookOpen size={24} className="mx-auto mb-2 text-white/10" />
              <p className="text-xs text-white/25">No memories found</p>
            </div>
          )}
        </div>
      </div>

      {/* Right panel - content viewer */}
      <div className="flex-1 h-full overflow-y-auto">
        {selected ? (
          <div className="p-6">
            <div className="flex items-center gap-3 mb-4">
              <FileText size={16} className="text-[#00d4ff]/60" />
              <h2 className="text-sm font-medium text-white/80">{selected}</h2>
              {files.find(f => f.filename === selected)?.type && (
                <TypeBadge type={files.find(f => f.filename === selected)!.type} />
              )}
            </div>
            {loading ? (
              <div className="text-xs text-white/25">Loading...</div>
            ) : (
              <pre className="text-xs text-white/60 whitespace-pre-wrap font-mono leading-relaxed bg-white/[0.02] rounded-lg p-4 border border-white/5">
                {content}
              </pre>
            )}
          </div>
        ) : (
          <div className="h-full flex items-center justify-center">
            <div className="text-center">
              <BookOpen size={32} className="mx-auto mb-3 text-white/8" />
              <p className="text-sm text-white/20">Select a memory to view</p>
              <p className="text-[11px] text-white/10 mt-1">Memories are Mimi's persistent knowledge</p>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

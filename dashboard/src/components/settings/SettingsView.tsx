import { useState, useEffect } from 'react'
import type { Status } from '../../hooks/useApi'
import { launchSession, stopSession, getConfig, saveConfig, createBackup } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Play, Square, Download } from 'lucide-react'

export function SettingsView({ status, onRefresh }: { status: Status | null; onRefresh: () => void }) {
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
    <div className="space-y-4">
      <div className="surface p-5">
        <div className="eyebrow mb-3">Session</div>
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div className="flex items-center gap-3">
            <span
              className={`w-2 h-2 rounded-full ${status?.session_running ? 'bg-success' : 'bg-danger'}`}
              style={{ boxShadow: status?.session_running ? '0 0 8px var(--success)' : '0 0 8px var(--danger)' }}
            />
            <span className="text-[13px]">
              {status?.session_running ? 'Running' : 'Stopped'}
            </span>
            {status?.claude_version && (
              <span className="text-[11px] text-muted-foreground font-mono">{status.claude_version}</span>
            )}
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              className="h-8 px-3 text-[12px]"
              onClick={async () => { await launchSession(); setTimeout(onRefresh, 1000) }}
            >
              <Play size={12} className="mr-1.5" />
              Launch
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="h-8 px-3 text-[12px] text-danger hover:text-danger"
              onClick={async () => { await stopSession(); setTimeout(onRefresh, 500) }}
            >
              <Square size={12} className="mr-1.5" />
              Stop
            </Button>
          </div>
        </div>
      </div>

      <div className="surface p-5">
        <div className="eyebrow mb-3">Configuration</div>
        <Textarea
          className="font-mono text-[12px] min-h-[220px] bg-muted/40 border-border mb-3"
          value={json}
          onChange={e => { setJson(e.target.value); setSaved(false); setError(null) }}
        />
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={handleSave}>Save config</Button>
          {saved && <span className="text-[11px] text-success">Saved</span>}
          {error && <span className="text-[11px] text-danger">{error}</span>}
        </div>
        <p className="text-[11px] text-muted-foreground mt-3">
          Changes to session_name or model require a relaunch.
        </p>
      </div>

      <div className="surface p-5">
        <div className="eyebrow mb-3">Backup</div>
        <Button
          size="sm"
          variant="outline"
          className="h-8 px-3 text-[12px]"
          onClick={async () => { await createBackup() }}
        >
          <Download size={12} className="mr-1.5" />
          Create backup
        </Button>
        <p className="text-[11px] text-muted-foreground mt-2">
          Snapshots ~/.mimi/ to a timestamped tar.gz archive.
        </p>
      </div>
    </div>
  )
}

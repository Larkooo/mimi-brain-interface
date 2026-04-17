import { useState, useEffect } from 'react'
import type { Status } from '../../hooks/useApi'
import { launchSession, stopSession, getConfig, saveConfig, createBackup } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Settings, Play, Square, Download } from 'lucide-react'

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
    <div className="max-w-2xl mx-auto py-10 px-6">
      <div className="flex items-center gap-3 mb-8">
        <Settings size={20} className="text-[#00d4ff]" />
        <h1 className="text-lg font-medium text-white/90">Settings</h1>
      </div>

      {/* Session control */}
      <div className="glass p-5 mb-6">
        <div className="text-[10px] text-white/30 uppercase tracking-wider mb-3">Session</div>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div
              className="w-2.5 h-2.5 rounded-full"
              style={{
                backgroundColor: status?.session_running ? '#00ffa3' : '#ff3d3d',
                boxShadow: status?.session_running ? '0 0 8px #00ffa3' : '0 0 8px #ff3d3d',
              }}
            />
            <span className="text-sm text-white/70">
              {status?.session_running ? 'Running' : 'Stopped'}
            </span>
            {status?.claude_version && (
              <span className="text-[10px] text-white/25 font-mono">{status.claude_version}</span>
            )}
          </div>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              className="border-white/10 text-white/60 hover:text-white/90"
              onClick={async () => { await launchSession(); setTimeout(onRefresh, 1000) }}
            >
              <Play size={12} className="mr-1.5" />
              Launch
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="border-white/10 text-red-400/60 hover:text-red-400"
              onClick={async () => { await stopSession(); setTimeout(onRefresh, 500) }}
            >
              <Square size={12} className="mr-1.5" />
              Stop
            </Button>
          </div>
        </div>
      </div>

      {/* Config editor */}
      <div className="glass p-5 mb-6">
        <div className="text-[10px] text-white/30 uppercase tracking-wider mb-3">Configuration</div>
        <Textarea
          className="font-mono text-sm min-h-[200px] bg-white/3 border-white/8 text-white/80 mb-3"
          value={json}
          onChange={e => { setJson(e.target.value); setSaved(false); setError(null) }}
        />
        <div className="flex items-center gap-3">
          <Button size="sm" onClick={handleSave}>Save Config</Button>
          {saved && <span className="text-xs text-[#00ffa3]">Saved</span>}
          {error && <span className="text-xs text-red-400">{error}</span>}
        </div>
        <p className="text-[10px] text-white/20 mt-3">
          Changes to session_name or model require a relaunch.
        </p>
      </div>

      {/* Backup */}
      <div className="glass p-5">
        <div className="text-[10px] text-white/30 uppercase tracking-wider mb-3">Backup</div>
        <Button
          size="sm"
          variant="outline"
          className="border-white/10 text-white/60 hover:text-white/90"
          onClick={async () => { await createBackup() }}
        >
          <Download size={12} className="mr-1.5" />
          Create Backup
        </Button>
        <p className="text-[10px] text-white/20 mt-2">
          Backs up ~/.mimi/ to a timestamped tar.gz archive.
        </p>
      </div>
    </div>
  )
}

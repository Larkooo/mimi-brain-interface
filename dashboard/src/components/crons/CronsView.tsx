import { useState, useEffect, useCallback } from 'react'
import type { CronJob } from '../../hooks/useApi'
import { getCrons, createCron, deleteCron, toggleCron } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Clock, Plus, Trash2, Power } from 'lucide-react'

const SCHEDULE_PRESETS = [
  { label: 'Every 5 min', value: '*/5 * * * *' },
  { label: 'Every 10 min', value: '*/10 * * * *' },
  { label: 'Every 30 min', value: '*/30 * * * *' },
  { label: 'Hourly', value: '0 * * * *' },
  { label: 'Daily 3am', value: '0 3 * * *' },
  { label: 'Custom', value: '' },
]

export function CronsView() {
  const [crons, setCrons] = useState<CronJob[]>([])
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [schedule, setSchedule] = useState('*/10 * * * *')
  const [customSchedule, setCustomSchedule] = useState('')
  const [prompt, setPrompt] = useState('')
  const [description, setDescription] = useState('')

  const refresh = useCallback(async () => {
    try { setCrons(await getCrons()) } catch {}
  }, [])

  useEffect(() => { refresh() }, [refresh])

  const handleCreate = async () => {
    const sched = schedule || customSchedule
    if (!name.trim() || !sched.trim() || !prompt.trim()) return
    await createCron(name, sched, prompt, description)
    setName(''); setSchedule('*/10 * * * *'); setCustomSchedule(''); setPrompt(''); setDescription('')
    setAdding(false)
    refresh()
  }

  const handleDelete = async (id: string) => {
    await deleteCron(id)
    refresh()
  }

  const handleToggle = async (id: string) => {
    await toggleCron(id)
    refresh()
  }

  return (
    <div className="max-w-2xl mx-auto py-10 px-6">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center gap-3">
          <Clock size={20} className="text-[#00d4ff]" />
          <h1 className="text-lg font-medium text-white/90">Cron Jobs</h1>
        </div>
        <Button
          size="sm"
          variant="outline"
          className="border-white/10 text-white/60 hover:text-white/90"
          onClick={() => setAdding(!adding)}
        >
          <Plus size={14} className="mr-1.5" />
          Add
        </Button>
      </div>

      {/* Info about how crons work */}
      <div className="glass px-4 py-3 mb-6 flex items-start gap-3">
        <Clock size={16} className="text-[#00d4ff]/60 shrink-0 mt-0.5" />
        <p className="text-xs text-white/40">
          Cron jobs are prompts that Mimi executes on a schedule using her built-in scheduler.
          They run while the session is active and auto-sync on startup.
        </p>
      </div>

      {adding && (
        <div className="glass p-5 mb-6 space-y-4">
          <Input
            placeholder="Job name (e.g. beeper-summary)"
            value={name}
            onChange={e => setName(e.target.value)}
            className="bg-white/5 border-white/10 text-white/80"
          />

          <div>
            <div className="text-[10px] text-white/30 uppercase tracking-wider mb-2">Schedule</div>
            <div className="flex flex-wrap gap-1.5 mb-2">
              {SCHEDULE_PRESETS.map(p => (
                <button
                  key={p.label}
                  onClick={() => setSchedule(p.value)}
                  className={`px-2.5 py-1 rounded-md text-xs transition-all ${
                    schedule === p.value
                      ? 'bg-[#00d4ff]/15 text-[#00d4ff] border border-[#00d4ff]/30'
                      : 'bg-white/5 text-white/40 border border-white/8 hover:text-white/60'
                  }`}
                >
                  {p.label}
                </button>
              ))}
            </div>
            {!schedule && (
              <Input
                placeholder="Custom cron expression (e.g. 0 */2 * * *)"
                value={customSchedule}
                onChange={e => setCustomSchedule(e.target.value)}
                className="bg-white/5 border-white/10 text-white/80 font-mono text-sm"
              />
            )}
            {schedule && (
              <div className="text-xs text-white/25 font-mono">{schedule}</div>
            )}
          </div>

          <div>
            <div className="text-[10px] text-white/30 uppercase tracking-wider mb-2">Prompt</div>
            <Textarea
              placeholder="What should Mimi do on this schedule? (e.g. Check Beeper for unread messages and send a summary to Telegram)"
              value={prompt}
              onChange={e => setPrompt(e.target.value)}
              className="bg-white/5 border-white/10 text-white/80 text-sm min-h-[100px]"
            />
          </div>

          <Input
            placeholder="Description (optional)"
            value={description}
            onChange={e => setDescription(e.target.value)}
            className="bg-white/5 border-white/10 text-white/80"
          />

          <div className="flex gap-2">
            <Button size="sm" onClick={handleCreate}
              disabled={!name.trim() || !(schedule || customSchedule).trim() || !prompt.trim()}>
              Create Cron
            </Button>
            <Button size="sm" variant="ghost" className="text-white/40" onClick={() => setAdding(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {crons.length === 0 && !adding ? (
        <div className="text-center py-20">
          <Clock size={32} className="mx-auto mb-3 text-white/10" />
          <p className="text-sm text-white/30">No cron jobs configured</p>
          <p className="text-xs text-white/15 mt-1">Add a cron job to schedule recurring tasks</p>
        </div>
      ) : (
        <div className="space-y-2">
          {crons.map(cron => (
            <div key={cron.id} className="glass px-4 py-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3 min-w-0">
                  <div
                    className="w-2 h-2 rounded-full shrink-0"
                    style={{
                      backgroundColor: cron.enabled ? '#00ffa3' : '#ff3d3d',
                      boxShadow: cron.enabled ? '0 0 6px #00ffa3' : '0 0 6px #ff3d3d',
                    }}
                  />
                  <div className="min-w-0">
                    <span className="text-sm font-medium text-white/80">{cron.name}</span>
                    <span className="ml-2 text-[10px] text-[#00d4ff]/50 font-mono">{cron.schedule}</span>
                  </div>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    className={`p-1.5 transition-colors ${cron.enabled ? 'text-[#00ffa3]/60 hover:text-[#00ffa3]' : 'text-white/25 hover:text-white/60'}`}
                    onClick={() => handleToggle(cron.id)}
                    title={cron.enabled ? 'Disable' : 'Enable'}
                  >
                    <Power size={14} />
                  </button>
                  <button
                    className="p-1.5 text-white/25 hover:text-red-400 transition-colors"
                    onClick={() => handleDelete(cron.id)}
                    title="Delete"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
              {cron.description && (
                <p className="text-[11px] text-white/25 mt-1 ml-5">{cron.description}</p>
              )}
              <div className="text-[11px] text-white/20 mt-1.5 ml-5 line-clamp-2">{cron.prompt}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

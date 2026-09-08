import { useState, useEffect, useCallback } from 'react'
import type { CronJob } from '../../hooks/useApi'
import { getCrons, createCron, deleteCron, toggleCron, runCron } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { Plus, Trash2, Power, Clock, Play } from 'lucide-react'

const SCHEDULE_PRESETS = [
  { label: 'Every 5 min', value: '*/5 * * * *' },
  { label: 'Every 10 min', value: '*/10 * * * *' },
  { label: 'Every 30 min', value: '*/30 * * * *' },
  { label: 'Hourly', value: '0 * * * *' },
  { label: 'Daily 3am', value: '0 3 * * *' },
  { label: 'Custom', value: '' },
]

function formatAge(iso: string): string {
  const secs = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 1000))
  if (secs < 60) return `${secs}s ago`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`
  return `${Math.floor(secs / 86400)}d ago`
}

export function CronsView() {
  const [crons, setCrons] = useState<CronJob[]>([])
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [schedule, setSchedule] = useState('*/10 * * * *')
  const [customSchedule, setCustomSchedule] = useState('')
  const [prompt, setPrompt] = useState('')
  const [description, setDescription] = useState('')
  const [error, setError] = useState('')

  const refresh = useCallback(async () => {
    try { setCrons(await getCrons()) } catch {}
  }, [])

  useEffect(() => { refresh() }, [refresh])

  const handleCreate = async () => {
    const sched = schedule || customSchedule
    if (!name.trim() || !sched.trim() || !prompt.trim()) return
    try {
      await createCron(name, sched, prompt, description)
    } catch (e) {
      // The API rejects expressions the scheduler can't run, so the job never
      // silently sits there never firing.
      setError(e instanceof Error ? e.message : 'Failed to create schedule')
      return
    }
    setError('')
    setName(''); setSchedule('*/10 * * * *'); setCustomSchedule(''); setPrompt(''); setDescription('')
    setAdding(false)
    refresh()
  }

  const handleRun = async (id: string) => {
    await runCron(id)
    // The run is async server-side; give it a beat before re-reading status.
    setTimeout(refresh, 1500)
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
    <div>
      <div className="flex items-center justify-between mb-6">
        <div className="text-[12px] text-muted-foreground">
          {crons.length} {crons.length === 1 ? 'job' : 'jobs'} configured
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setAdding(!adding)}
          className="h-8 px-3 text-[12px]"
        >
          <Plus size={13} className="mr-1.5" />
          New schedule
        </Button>
      </div>

      {adding && (
        <div className="surface p-5 mb-5 space-y-4">
          <Input
            placeholder="Job name (e.g. beeper-summary)"
            value={name}
            onChange={e => setName(e.target.value)}
            className="bg-muted/40 border-border"
          />

          <div>
            <div className="eyebrow mb-2">Schedule</div>
            <div className="flex flex-wrap gap-1.5 mb-2">
              {SCHEDULE_PRESETS.map(p => (
                <button
                  key={p.label}
                  onClick={() => setSchedule(p.value)}
                  className={[
                    'px-2.5 py-1 rounded-md text-[11px] border transition-colors',
                    schedule === p.value
                      ? 'border-border text-foreground bg-accent'
                      : 'border-border/60 text-muted-foreground hover:text-foreground hover:bg-accent/60',
                  ].join(' ')}
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
                className="bg-muted/40 border-border font-mono text-sm"
              />
            )}
            {schedule && (
              <div className="text-[11px] text-muted-foreground/70 font-mono">{schedule}</div>
            )}
          </div>

          <div>
            <div className="eyebrow mb-2">Prompt</div>
            <Textarea
              placeholder="What should Mimi do? (e.g. Check Beeper for unread messages and DM a summary)"
              value={prompt}
              onChange={e => setPrompt(e.target.value)}
              className="bg-muted/40 border-border text-sm min-h-[100px]"
            />
          </div>

          <Input
            placeholder="Description (optional)"
            value={description}
            onChange={e => setDescription(e.target.value)}
            className="bg-muted/40 border-border"
          />

          {error && (
            <p className="text-[11px] text-danger">{error}</p>
          )}

          <div className="flex gap-2">
            <Button size="sm" onClick={handleCreate}
              disabled={!name.trim() || !(schedule || customSchedule).trim() || !prompt.trim()}>
              Create schedule
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAdding(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {crons.length === 0 && !adding ? (
        <div className="surface text-center py-16 px-6">
          <Clock size={28} strokeWidth={1.4} className="mx-auto mb-3 text-muted-foreground/50" />
          <p className="text-[13px] text-muted-foreground">No schedules configured</p>
          <p className="text-[11px] text-muted-foreground/60 mt-1">Add one to have Mimi run a prompt on a recurring cadence.</p>
        </div>
      ) : (
        <div className="space-y-2">
          {crons.map(cron => (
            <div key={cron.id} className="surface px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-3 min-w-0 flex-1">
                  <span
                    className={`w-1.5 h-1.5 rounded-full shrink-0 ${cron.enabled ? 'bg-success' : 'bg-muted-foreground/40'}`}
                    style={cron.enabled ? { boxShadow: '0 0 6px var(--success)' } : undefined}
                  />
                  <div className="min-w-0 flex items-baseline gap-2 flex-wrap">
                    <span className="text-[13px] font-medium tracking-tight">{cron.name}</span>
                    <span className="text-[10px] text-muted-foreground font-mono">{cron.schedule}</span>
                  </div>
                </div>
                <div className="flex items-center gap-0.5 shrink-0">
                  <button
                    className="p-1.5 rounded-md text-muted-foreground/60 hover:text-foreground hover:bg-accent transition-colors"
                    onClick={() => handleRun(cron.id)}
                    title="Run now"
                  >
                    <Play size={13} />
                  </button>
                  <button
                    className={`p-1.5 rounded-md transition-colors ${cron.enabled ? 'text-success hover:bg-accent' : 'text-muted-foreground/60 hover:text-foreground hover:bg-accent'}`}
                    onClick={() => handleToggle(cron.id)}
                    title={cron.enabled ? 'Disable' : 'Enable'}
                  >
                    <Power size={13} />
                  </button>
                  <button
                    className="p-1.5 rounded-md text-muted-foreground/60 hover:text-danger hover:bg-accent transition-colors"
                    onClick={() => handleDelete(cron.id)}
                    title="Delete"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
              {cron.description && (
                <p className="text-[11px] text-muted-foreground mt-1.5 ml-4">{cron.description}</p>
              )}
              <div className="text-[11px] text-muted-foreground/70 mt-1 ml-4 line-clamp-2">{cron.prompt}</div>
              <div className="text-[10px] text-muted-foreground/60 mt-1.5 ml-4">
                {cron.last_run ? (
                  <>
                    last run {formatAge(cron.last_run)}
                    {cron.last_status && (
                      <span className={cron.last_status === 'ok' || cron.last_status === 'running'
                        ? 'text-muted-foreground/60'
                        : 'text-danger'}> · {cron.last_status}</span>
                    )}
                  </>
                ) : (
                  'never run'
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

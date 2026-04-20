import { useEffect, useState } from 'react'
import type { GraphData } from './hooks/useApi'
import { getGraph, useStatus } from './hooks/useApi'
import { NavRail } from './components/NavRail'
import type { View } from './components/NavRail'
import { HomeView } from './components/home/HomeView'
import { BrainView } from './components/brain/BrainView'
import { MemoryView } from './components/memory/MemoryView'
import { ChannelsView } from './components/channels/ChannelsView'
import { CronsView } from './components/crons/CronsView'
import { SecretsView } from './components/secrets/SecretsView'
import { SettingsView } from './components/settings/SettingsView'
import { LogsView } from './components/logviewer/LogsView'
import { ServicesView } from './components/services/ServicesView'
import { NutritionView } from './components/nutrition/NutritionView'
import { TasksView } from './components/tasks/TasksView'

const TITLES: Record<Exclude<View, 'home' | 'brain' | 'logs' | 'services' | 'nutrition' | 'memory' | 'tasks'>, { title: string; subtitle: string }> = {
  channels: { title: 'channels',  subtitle: 'inbound bridges mimi answers on.' },
  crons:    { title: 'schedules', subtitle: 'recurring prompts the scheduler fires on mimi.' },
  secrets:  { title: 'secrets',   subtitle: 'encrypted keystore — values never enter context.' },
  settings: { title: 'settings',  subtitle: 'runtime config and identity.' },
}

function App() {
  const { status, refresh } = useStatus()
  const [view, setView] = useState<View>('home')
  const [graph, setGraph] = useState<GraphData | null>(null)

  useEffect(() => {
    if (view !== 'brain') return
    let cancelled = false
    const load = async () => {
      try {
        const g = await getGraph()
        if (!cancelled) setGraph(g)
      } catch {
        if (!cancelled) setGraph(null)
      }
    }
    load()
    const t = setInterval(load, 15000)
    return () => { cancelled = true; clearInterval(t) }
  }, [view])

  return (
    <div className="min-h-screen text-foreground font-mono relative">
      <NavRail active={view} onChange={setView} />
      <main className="relative min-h-screen pt-11">
        {view === 'home' && <HomeView status={status} />}
        {view === 'brain' && <BrainView status={status} graph={graph} />}
        {view === 'logs' && <LogsView />}
        {view === 'services' && <ServicesView />}
        {view === 'nutrition' && <NutritionView />}
        {view === 'memory' && <MemoryView />}
        {view === 'tasks' && <TasksView />}
        {(view === 'channels' || view === 'crons' || view === 'secrets' || view === 'settings') && (
          <PageShell {...TITLES[view]}>
            {view === 'channels' && <ChannelsView channels={status?.channels ?? []} onRefresh={refresh} />}
            {view === 'crons'    && <CronsView />}
            {view === 'secrets'  && <SecretsView />}
            {view === 'settings' && <SettingsView status={status} onRefresh={refresh} />}
          </PageShell>
        )}
      </main>
    </div>
  )
}

function PageShell({ title, subtitle, children }: { title: string; subtitle: string; children: React.ReactNode }) {
  return (
    <div className="px-8 pt-10 pb-20 max-w-6xl mx-auto">
      <header className="mb-10">
        <div className="flex items-baseline gap-2 mb-1">
          <span style={{ color: 'var(--accentphosphor)' }}>&gt;</span>
          <h1 className="text-[20px] font-semibold tracking-wide lowercase">{title}</h1>
        </div>
        <p className="text-[13px] text-muted-foreground ml-4">{subtitle}</p>
      </header>
      {children}
    </div>
  )
}

export default App

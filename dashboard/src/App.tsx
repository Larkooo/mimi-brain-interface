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

const TITLES: Record<Exclude<View, 'home' | 'brain' | 'logs' | 'services' | 'nutrition' | 'memory'>, { title: string; subtitle: string }> = {
  channels: { title: 'Channels',  subtitle: 'Inbound bridges Mimi answers on.' },
  crons:    { title: 'Schedules', subtitle: 'Recurring prompts the scheduler fires on Mimi.' },
  secrets:  { title: 'Secrets',   subtitle: 'API keys, tokens, and credentials in the keystore.' },
  settings: { title: 'Settings',  subtitle: 'Runtime config and identity.' },
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
    <div className="min-h-screen bg-background text-foreground font-sans">
      <NavRail active={view} onChange={setView} />
      <main className="pl-[220px] relative min-h-screen">
        {view === 'home' && <HomeView status={status} />}
        {view === 'brain' && <BrainView status={status} graph={graph} />}
        {view === 'logs' && <LogsView />}
        {view === 'services' && <ServicesView />}
        {view === 'nutrition' && <NutritionView />}
        {view === 'memory' && <MemoryView />}
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
    <div className="px-10 py-10 max-w-5xl mx-auto">
      <header className="mb-8">
        <div className="eyebrow mb-1.5">Section</div>
        <h1 className="text-[26px] font-semibold tracking-tight leading-none">{title}</h1>
        <p className="text-sm text-muted-foreground mt-2">{subtitle}</p>
      </header>
      {children}
    </div>
  )
}

export default App

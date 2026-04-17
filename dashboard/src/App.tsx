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
      {view === 'home' && <HomeView status={status} />}
      {view === 'brain' && <BrainView status={status} graph={graph} />}
      {view === 'memory' && <WithHeader title="Memory"><MemoryView /></WithHeader>}
      {view === 'channels' && <WithHeader title="Channels"><ChannelsView channels={status?.channels ?? []} onRefresh={refresh} /></WithHeader>}
      {view === 'crons' && <WithHeader title="Crons"><CronsView /></WithHeader>}
      {view === 'secrets' && <WithHeader title="Secrets"><SecretsView /></WithHeader>}
      {view === 'logs' && <LogsView />}
      {view === 'services' && <ServicesView />}
      {view === 'settings' && <WithHeader title="Settings"><SettingsView status={status} onRefresh={refresh} /></WithHeader>}
    </div>
  )
}

function WithHeader({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="p-8 pl-20 max-w-5xl mx-auto">
      <h1 className="text-2xl font-semibold tracking-tight mb-6">{title}</h1>
      {children}
    </div>
  )
}

export default App

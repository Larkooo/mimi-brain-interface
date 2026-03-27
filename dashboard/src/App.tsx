import { useState } from 'react'
import { useStatus } from './hooks/useApi'
import { Header } from './components/Header'
import { StatusPanel } from './components/StatusPanel'
import { BrainPanel } from './components/BrainPanel'
import { ChannelsPanel } from './components/ChannelsPanel'
import { QueryPanel } from './components/QueryPanel'
import { ConfigPanel } from './components/ConfigPanel'
import './App.css'

type Tab = 'overview' | 'brain' | 'channels' | 'query' | 'config'

function App() {
  const { status, refresh } = useStatus()
  const [tab, setTab] = useState<Tab>('overview')

  return (
    <div className="app">
      <Header status={status} />
      <nav className="tabs">
        {(['overview', 'brain', 'channels', 'query', 'config'] as Tab[]).map(t => (
          <button
            key={t}
            className={`tab ${tab === t ? 'active' : ''}`}
            onClick={() => setTab(t)}
          >
            {t}
          </button>
        ))}
      </nav>
      <main className="content">
        {tab === 'overview' && <StatusPanel status={status} onRefresh={refresh} />}
        {tab === 'brain' && <BrainPanel stats={status?.brain_stats} />}
        {tab === 'channels' && <ChannelsPanel channels={status?.channels ?? []} onRefresh={refresh} />}
        {tab === 'query' && <QueryPanel />}
        {tab === 'config' && <ConfigPanel />}
      </main>
      <div className="scanline" />
    </div>
  )
}

export default App

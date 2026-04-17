import { useState } from 'react'
import { useStatus } from './hooks/useApi'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Header } from './components/Header'
import { StatusPanel } from './components/StatusPanel'
import { BrainPanel } from './components/BrainPanel'
import { ChannelsPanel } from './components/ChannelsPanel'
import { QueryPanel } from './components/QueryPanel'
import { ConfigPanel } from './components/ConfigPanel'

function App() {
  const { status, error, refresh } = useStatus()
  const [_tab, setTab] = useState('overview')

  return (
    <div className="min-h-screen bg-background font-sans">
      <Header status={status} error={error} />
      <div className="max-w-6xl mx-auto px-6 py-6">
        <Tabs defaultValue="overview" onValueChange={setTab}>
          <TabsList className="mb-6">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="brain">Brain</TabsTrigger>
            <TabsTrigger value="channels">Channels</TabsTrigger>
            <TabsTrigger value="query">Query</TabsTrigger>
            <TabsTrigger value="config">Config</TabsTrigger>
          </TabsList>
          <TabsContent value="overview">
            <StatusPanel status={status} error={error} onRefresh={refresh} />
          </TabsContent>
          <TabsContent value="brain">
            <BrainPanel stats={status?.brain_stats} />
          </TabsContent>
          <TabsContent value="channels">
            <ChannelsPanel channels={status?.channels ?? []} onRefresh={refresh} />
          </TabsContent>
          <TabsContent value="query">
            <QueryPanel />
          </TabsContent>
          <TabsContent value="config">
            <ConfigPanel />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  )
}

export default App

import type { Status } from '../hooks/useApi'
import { launchSession, stopSession, createBackup } from '../hooks/useApi'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'

export function StatusPanel({ status, onRefresh }: { status: Status | null; onRefresh: () => void }) {
  if (!status) return <p className="text-muted-foreground">Connecting...</p>

  const bs = status.brain_stats

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Session</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold font-mono mb-3" style={{ color: status.session_running ? 'var(--chart-1)' : 'hsl(var(--destructive))' }}>
              {status.session_running ? 'ON' : 'OFF'}
            </div>
            <div className="flex gap-2">
              <Button size="sm" onClick={async () => { await launchSession(); setTimeout(onRefresh, 1000); }}>
                Launch
              </Button>
              <Button size="sm" variant="destructive" onClick={async () => { await stopSession(); setTimeout(onRefresh, 500); }}>
                Stop
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Knowledge Graph</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex gap-6 mb-3">
              <div>
                <div className="text-3xl font-bold font-mono text-blue-400">{bs.entities}</div>
                <div className="text-xs text-muted-foreground">entities</div>
              </div>
              <div>
                <div className="text-3xl font-bold font-mono text-purple-400">{bs.relationships}</div>
                <div className="text-xs text-muted-foreground">links</div>
              </div>
            </div>
            {bs.entity_types.length > 0 && (
              <>
                <Separator className="my-2" />
                <div className="flex flex-wrap gap-1.5">
                  {bs.entity_types.map(([type, count]) => (
                    <Badge key={type} variant="secondary" className="font-mono text-xs">
                      {type}: {count}
                    </Badge>
                  ))}
                </div>
              </>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Memory</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-3xl font-bold font-mono mb-3">{status.memory_files}</div>
            <div className="text-xs text-muted-foreground mb-3">memory files</div>
            <Button size="sm" variant="outline" onClick={async () => { await createBackup(); }}>
              Backup
            </Button>
          </CardContent>
        </Card>
      </div>

      {status.channels.length > 0 && (
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Active Channels</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex flex-wrap gap-2">
              {status.channels.map(ch => (
                <Badge key={ch.name} variant={ch.enabled ? 'default' : 'destructive'}>
                  {ch.name}
                </Badge>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

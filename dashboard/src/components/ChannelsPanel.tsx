import { useState } from 'react'
import type { Channel } from '../hooks/useApi'
import { addChannel, removeChannel, toggleChannel, configureChannel } from '../hooks/useApi'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'

const CHANNEL_INFO: Record<string, { tokenLabel: string; instructions: string[] }> = {
  telegram: {
    tokenLabel: 'Bot Token (from @BotFather)',
    instructions: [
      'Open @BotFather on Telegram',
      'Send /newbot and follow the prompts',
      'Copy the token (looks like 123456789:AAHfiqk...)',
      'Paste it below',
    ],
  },
  discord: {
    tokenLabel: 'Bot Token (from Developer Portal)',
    instructions: [
      'Go to discord.com/developers, create New Application',
      'Go to Bot, Reset Token, Copy',
      'Enable Message Content Intent',
      'Paste token below',
    ],
  },
}

export function ChannelsPanel({ channels, onRefresh }: { channels: Channel[]; onRefresh: () => void }) {
  const [newType, setNewType] = useState('telegram')
  const [token, setToken] = useState('')
  const [configuring, setConfiguring] = useState<string | null>(null)
  const [configToken, setConfigToken] = useState('')

  const needsToken = newType !== 'imessage'
  const info = CHANNEL_INFO[newType]

  const handleAdd = async () => {
    await addChannel(newType, needsToken ? token : undefined)
    setToken('')
    onRefresh()
  }

  const handleConfigure = async (name: string) => {
    if (!configToken.trim()) return
    await configureChannel(name, configToken)
    setConfiguring(null)
    setConfigToken('')
    onRefresh()
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Configured Channels</CardTitle>
        </CardHeader>
        <CardContent>
          {channels.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-4">No channels configured. Add one below.</p>
          ) : (
            <div className="space-y-2">
              {channels.map(ch => (
                <div key={ch.name}>
                  <div className="flex items-center justify-between py-2">
                    <div className="flex items-center gap-3">
                      <div className={`w-2 h-2 rounded-full ${ch.enabled ? 'bg-emerald-500' : 'bg-red-500'}`} />
                      <span className="font-medium">{ch.name}</span>
                      <Badge variant="secondary">{ch.type}</Badge>
                      {ch.plugin && <span className="text-xs text-muted-foreground font-mono">{ch.plugin}</span>}
                    </div>
                    <div className="flex gap-2">
                      {(ch.type === 'telegram' || ch.type === 'discord') && (
                        <Button size="sm" variant="outline" onClick={() => {
                          setConfiguring(configuring === ch.name ? null : ch.name)
                          setConfigToken('')
                        }}>
                          Configure
                        </Button>
                      )}
                      <Button size="sm" variant={ch.enabled ? 'destructive' : 'default'}
                        onClick={async () => { await toggleChannel(ch.name); onRefresh(); }}>
                        {ch.enabled ? 'Disable' : 'Enable'}
                      </Button>
                      <Button size="sm" variant="destructive"
                        onClick={async () => { await removeChannel(ch.name); onRefresh(); }}>
                        Remove
                      </Button>
                    </div>
                  </div>
                  {configuring === ch.name && (
                    <div className="pl-5 pb-3 flex gap-2">
                      <Input
                        type="password"
                        placeholder={`${ch.type} bot token...`}
                        value={configToken}
                        onChange={e => setConfigToken(e.target.value)}
                        onKeyDown={e => { if (e.key === 'Enter') handleConfigure(ch.name) }}
                      />
                      <Button size="sm" onClick={() => handleConfigure(ch.name)}>Save Token</Button>
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Add Channel</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <select className="flex h-9 w-auto rounded-md border border-input bg-transparent px-3 py-1 text-sm" value={newType} onChange={e => setNewType(e.target.value)}>
            <option value="telegram">Telegram</option>
            <option value="discord">Discord</option>
            <option value="imessage">iMessage</option>
          </select>

          {info && (
            <ol className="text-sm text-muted-foreground space-y-1 list-decimal list-inside">
              {info.instructions.map((step, i) => <li key={i}>{step}</li>)}
            </ol>
          )}

          {needsToken && (
            <Input
              type="password"
              placeholder={info?.tokenLabel || 'Bot token...'}
              value={token}
              onChange={e => setToken(e.target.value)}
            />
          )}

          <Button onClick={handleAdd} disabled={needsToken && !token.trim()}>
            Add Channel
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}

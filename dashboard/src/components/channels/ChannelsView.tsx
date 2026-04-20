import { useState } from 'react'
import type { Channel } from '../../hooks/useApi'
import { addChannel, removeChannel, toggleChannel, configureChannel } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Radio, Plus, Trash2, Settings, Power } from 'lucide-react'

const CHANNEL_INFO: Record<string, { tokenLabel: string; instructions: string[] }> = {
  telegram: {
    tokenLabel: 'Bot Token (from @BotFather)',
    instructions: [
      'Open @BotFather on Telegram',
      'Send /newbot and follow the prompts',
      'Copy the token',
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

export function ChannelsView({ channels, onRefresh }: { channels: Channel[]; onRefresh: () => void }) {
  const [newType, setNewType] = useState('telegram')
  const [token, setToken] = useState('')
  const [configuring, setConfiguring] = useState<string | null>(null)
  const [configToken, setConfigToken] = useState('')
  const [adding, setAdding] = useState(false)

  const needsToken = newType !== 'imessage'
  const info = CHANNEL_INFO[newType]

  const handleAdd = async () => {
    await addChannel(newType, needsToken ? token : undefined)
    setToken('')
    setAdding(false)
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
    <div>
      <div className="flex items-center justify-between mb-6">
        <div className="text-[12px] text-muted-foreground">
          {channels.length} {channels.length === 1 ? 'channel' : 'channels'} configured
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setAdding(!adding)}
          className="h-8 px-3 text-[12px]"
        >
          <Plus size={13} className="mr-1.5" />
          New channel
        </Button>
      </div>

      {adding && (
        <div className="surface p-5 mb-5 space-y-4">
          <select
            className="w-full h-9 rounded-md bg-muted/40 border border-border px-3 text-sm"
            value={newType}
            onChange={e => setNewType(e.target.value)}
          >
            <option value="telegram">Telegram</option>
            <option value="discord">Discord</option>
            <option value="imessage">iMessage</option>
          </select>

          {info && (
            <ol className="text-[11px] text-muted-foreground space-y-1 list-decimal list-inside">
              {info.instructions.map((step, i) => <li key={i}>{step}</li>)}
            </ol>
          )}

          {needsToken && (
            <Input
              type="password"
              placeholder={info?.tokenLabel || 'Bot token...'}
              value={token}
              onChange={e => setToken(e.target.value)}
              className="bg-muted/40 border-border"
            />
          )}

          <div className="flex gap-2">
            <Button size="sm" onClick={handleAdd} disabled={needsToken && !token.trim()}>
              Add channel
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setAdding(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {channels.length === 0 && !adding ? (
        <div className="surface text-center py-16 px-6">
          <Radio size={28} strokeWidth={1.4} className="mx-auto mb-3 text-muted-foreground/50" />
          <p className="text-[13px] text-muted-foreground">No channels configured</p>
          <p className="text-[11px] text-muted-foreground/60 mt-1">Add a channel to bridge Mimi to a messaging platform.</p>
        </div>
      ) : (
        <div className="space-y-2">
          {channels.map(ch => (
            <div key={ch.name}>
              <div className="surface px-4 py-3 flex items-center justify-between">
                <div className="flex items-center gap-3 min-w-0">
                  <span
                    className={`w-1.5 h-1.5 rounded-full shrink-0 ${ch.enabled ? 'bg-success' : 'bg-muted-foreground/40'}`}
                    style={ch.enabled ? { boxShadow: '0 0 6px var(--success)' } : undefined}
                  />
                  <span className="text-[13px] font-medium tracking-tight truncate">{ch.name}</span>
                  <span className="text-[10px] text-muted-foreground font-mono uppercase">{ch.type}</span>
                </div>
                <div className="flex items-center gap-0.5 shrink-0">
                  {(ch.type === 'telegram' || ch.type === 'discord') && (
                    <button
                      className="p-1.5 rounded-md text-muted-foreground/60 hover:text-foreground hover:bg-accent transition-colors"
                      onClick={() => {
                        setConfiguring(configuring === ch.name ? null : ch.name)
                        setConfigToken('')
                      }}
                      title="Configure token"
                    >
                      <Settings size={13} />
                    </button>
                  )}
                  <button
                    className={`p-1.5 rounded-md transition-colors ${ch.enabled ? 'text-success hover:bg-accent' : 'text-muted-foreground/60 hover:text-foreground hover:bg-accent'}`}
                    onClick={async () => { await toggleChannel(ch.name); onRefresh() }}
                    title={ch.enabled ? 'Disable' : 'Enable'}
                  >
                    <Power size={13} />
                  </button>
                  <button
                    className="p-1.5 rounded-md text-muted-foreground/60 hover:text-danger hover:bg-accent transition-colors"
                    onClick={async () => { await removeChannel(ch.name); onRefresh() }}
                    title="Remove"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
              {configuring === ch.name && (
                <div className="px-4 pb-3 pt-1 flex gap-2">
                  <Input
                    type="password"
                    placeholder={`${ch.type} bot token...`}
                    value={configToken}
                    onChange={e => setConfigToken(e.target.value)}
                    onKeyDown={e => { if (e.key === 'Enter') handleConfigure(ch.name) }}
                    className="bg-muted/40 border-border text-sm"
                  />
                  <Button size="sm" onClick={() => handleConfigure(ch.name)}>Save</Button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

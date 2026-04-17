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
    <div className="max-w-2xl mx-auto py-10 px-6">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center gap-3">
          <Radio size={20} className="text-[#00d4ff]" />
          <h1 className="text-lg font-medium text-white/90">Channels</h1>
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

      {/* Add channel form */}
      {adding && (
        <div className="glass p-5 mb-6 space-y-4">
          <select
            className="w-full h-9 rounded-md bg-white/5 border border-white/10 px-3 text-sm text-white/80"
            value={newType}
            onChange={e => setNewType(e.target.value)}
          >
            <option value="telegram">Telegram</option>
            <option value="discord">Discord</option>
            <option value="imessage">iMessage</option>
          </select>

          {info && (
            <ol className="text-xs text-white/40 space-y-1 list-decimal list-inside">
              {info.instructions.map((step, i) => <li key={i}>{step}</li>)}
            </ol>
          )}

          {needsToken && (
            <Input
              type="password"
              placeholder={info?.tokenLabel || 'Bot token...'}
              value={token}
              onChange={e => setToken(e.target.value)}
              className="bg-white/5 border-white/10 text-white/80"
            />
          )}

          <div className="flex gap-2">
            <Button size="sm" onClick={handleAdd} disabled={needsToken && !token.trim()}>
              Add Channel
            </Button>
            <Button size="sm" variant="ghost" className="text-white/40" onClick={() => setAdding(false)}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {/* Channel list */}
      {channels.length === 0 && !adding ? (
        <div className="text-center py-20">
          <Radio size={32} className="mx-auto mb-3 text-white/10" />
          <p className="text-sm text-white/30">No channels configured</p>
          <p className="text-xs text-white/15 mt-1">Add a channel to connect Mimi to messaging platforms</p>
        </div>
      ) : (
        <div className="space-y-2">
          {channels.map(ch => (
            <div key={ch.name}>
              <div className="glass px-4 py-3 flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div
                    className="w-2 h-2 rounded-full"
                    style={{
                      backgroundColor: ch.enabled ? '#00ffa3' : '#ff3d3d',
                      boxShadow: ch.enabled ? '0 0 6px #00ffa3' : '0 0 6px #ff3d3d',
                    }}
                  />
                  <span className="text-sm font-medium text-white/80">{ch.name}</span>
                  <span className="text-[10px] text-white/25 font-mono uppercase">{ch.type}</span>
                </div>
                <div className="flex items-center gap-1">
                  {(ch.type === 'telegram' || ch.type === 'discord') && (
                    <button
                      className="p-1.5 text-white/25 hover:text-white/60 transition-colors"
                      onClick={() => {
                        setConfiguring(configuring === ch.name ? null : ch.name)
                        setConfigToken('')
                      }}
                      title="Configure token"
                    >
                      <Settings size={14} />
                    </button>
                  )}
                  <button
                    className={`p-1.5 transition-colors ${ch.enabled ? 'text-[#00ffa3]/60 hover:text-[#00ffa3]' : 'text-white/25 hover:text-white/60'}`}
                    onClick={async () => { await toggleChannel(ch.name); onRefresh() }}
                    title={ch.enabled ? 'Disable' : 'Enable'}
                  >
                    <Power size={14} />
                  </button>
                  <button
                    className="p-1.5 text-white/25 hover:text-red-400 transition-colors"
                    onClick={async () => { await removeChannel(ch.name); onRefresh() }}
                    title="Remove"
                  >
                    <Trash2 size={14} />
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
                    className="bg-white/5 border-white/10 text-white/80 text-sm"
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

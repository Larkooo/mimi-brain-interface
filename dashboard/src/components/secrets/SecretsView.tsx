import { useState, useEffect, useCallback } from 'react'
import type { SecretEntry } from '../../hooks/useApi'
import { getSecrets, setSecret, deleteSecret } from '../../hooks/useApi'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { KeyRound, Plus, Trash2, Eye, EyeOff, ShieldCheck } from 'lucide-react'

export function SecretsView() {
  const [secrets, setSecrets] = useState<SecretEntry[]>([])
  const [adding, setAdding] = useState(false)
  const [name, setName] = useState('')
  const [value, setValue] = useState('')
  const [showValue, setShowValue] = useState(false)

  const refresh = useCallback(async () => {
    try { setSecrets(await getSecrets()) } catch {}
  }, [])

  useEffect(() => { refresh() }, [refresh])

  const handleCreate = async () => {
    if (!name.trim() || !value.trim()) return
    await setSecret(name.trim(), value)
    setName(''); setValue(''); setShowValue(false)
    setAdding(false)
    refresh()
  }

  const handleDelete = async (secretName: string) => {
    await deleteSecret(secretName)
    refresh()
  }

  return (
    <div className="max-w-2xl mx-auto py-10 px-6">
      <div className="flex items-center justify-between mb-8">
        <div className="flex items-center gap-3">
          <KeyRound size={20} className="text-[#00d4ff]" />
          <h1 className="text-lg font-medium text-white/90">Secrets</h1>
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

      {/* Security notice */}
      <div className="glass px-4 py-3 mb-6 flex items-start gap-3">
        <ShieldCheck size={16} className="text-[#00ffa3]/60 shrink-0 mt-0.5" />
        <div>
          <p className="text-xs text-white/40">
            Secrets are <span className="text-[#00ffa3]/70">encrypted at rest</span> with AES-256-CBC. Values are <span className="text-[#00ffa3]/70">never returned through the API</span> and never enter the AI context window.
            Use <span className="font-mono text-white/30">mimi secret run</span> to inject them as environment variables into commands.
          </p>
        </div>
      </div>

      {adding && (
        <div className="glass p-5 mb-6 space-y-4">
          <Input
            placeholder="Secret name (e.g. OPENAI_API_KEY)"
            value={name}
            onChange={e => setName(e.target.value.replace(/[^a-zA-Z0-9_\-.]/, ''))}
            className="bg-white/5 border-white/10 text-white/80 font-mono"
          />

          <div className="relative">
            <Input
              type={showValue ? 'text' : 'password'}
              placeholder="Secret value..."
              value={value}
              onChange={e => setValue(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleCreate() }}
              className="bg-white/5 border-white/10 text-white/80 font-mono pr-10"
            />
            <button
              className="absolute right-2 top-1/2 -translate-y-1/2 text-white/25 hover:text-white/50 transition-colors"
              onClick={() => setShowValue(!showValue)}
            >
              {showValue ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
          </div>

          <div className="flex gap-2">
            <Button size="sm" onClick={handleCreate} disabled={!name.trim() || !value.trim()}>
              Save Secret
            </Button>
            <Button size="sm" variant="ghost" className="text-white/40" onClick={() => { setAdding(false); setValue('') }}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {secrets.length === 0 && !adding ? (
        <div className="text-center py-20">
          <KeyRound size={32} className="mx-auto mb-3 text-white/10" />
          <p className="text-sm text-white/30">No secrets stored</p>
          <p className="text-xs text-white/15 mt-1">Add API keys and credentials that stay out of the AI context</p>
        </div>
      ) : (
        <div className="space-y-2">
          {secrets.map(secret => (
            <div key={secret.name} className="glass px-4 py-3 flex items-center justify-between">
              <div className="flex items-center gap-3 min-w-0">
                <KeyRound size={14} className="text-white/20 shrink-0" />
                <div className="min-w-0">
                  <span className="text-sm font-mono text-white/80">{secret.name}</span>
                  {secret.created_at && (
                    <span className="ml-2 text-[10px] text-white/20">{secret.created_at}</span>
                  )}
                </div>
              </div>
              <button
                className="p-1.5 text-white/25 hover:text-red-400 transition-colors shrink-0"
                onClick={() => handleDelete(secret.name)}
                title="Delete"
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Usage instructions */}
      {secrets.length > 0 && (
        <div className="mt-8 glass px-4 py-3">
          <div className="text-[10px] text-white/30 uppercase tracking-wider mb-2">Usage</div>
          <pre className="text-[11px] text-white/25 font-mono">
{`# Run a command with a secret injected as env var
mimi secret run OPENAI_API_KEY OPENAI_API_KEY -- curl -H "Authorization: Bearer $OPENAI_API_KEY" ...

# Syntax: mimi secret run <secret_name> <env_var> -- <command...>
# The decrypted value never appears in stdout`}
          </pre>
        </div>
      )}
    </div>
  )
}

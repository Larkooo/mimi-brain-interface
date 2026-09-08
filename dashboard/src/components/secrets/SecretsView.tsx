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
    <div>
      <div className="flex items-center justify-between mb-6">
        <div className="text-[12px] text-muted-foreground">
          {secrets.length} {secrets.length === 1 ? 'secret' : 'secrets'} in keystore
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setAdding(!adding)}
          className="h-8 px-3 text-[12px]"
        >
          <Plus size={13} className="mr-1.5" />
          New secret
        </Button>
      </div>

      <div className="surface px-4 py-3 mb-5 flex items-start gap-3">
        <ShieldCheck size={15} className="text-success shrink-0 mt-0.5" />
        <p className="text-[12px] text-muted-foreground leading-relaxed">
          Secrets are encrypted at rest with AES-256-CBC. Values are never returned through the API and never enter the AI context window.
          Use <span className="font-mono text-foreground/80">mimi secret run</span> to inject them as environment variables.
        </p>
      </div>

      {adding && (
        <div className="surface p-5 mb-5 space-y-4">
          <Input
            placeholder="Secret name (e.g. OPENAI_API_KEY)"
            value={name}
            onChange={e => setName(e.target.value.replace(/[^a-zA-Z0-9_\-.]/g, '').replace(/\.{2,}/g, '.').replace(/^\.+/, ''))}
            className="bg-muted/40 border-border font-mono"
          />

          <div className="relative">
            <Input
              type={showValue ? 'text' : 'password'}
              placeholder="Secret value..."
              value={value}
              onChange={e => setValue(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter') handleCreate() }}
              className="bg-muted/40 border-border font-mono pr-10"
            />
            <button
              className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground transition-colors"
              onClick={() => setShowValue(!showValue)}
            >
              {showValue ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
          </div>

          <div className="flex gap-2">
            <Button size="sm" onClick={handleCreate} disabled={!name.trim() || !value.trim()}>
              Save secret
            </Button>
            <Button size="sm" variant="ghost" onClick={() => { setAdding(false); setValue('') }}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {secrets.length === 0 && !adding ? (
        <div className="surface text-center py-16 px-6">
          <KeyRound size={28} strokeWidth={1.4} className="mx-auto mb-3 text-muted-foreground/50" />
          <p className="text-[13px] text-muted-foreground">No secrets stored</p>
          <p className="text-[11px] text-muted-foreground/60 mt-1">API keys and credentials kept out of the AI context.</p>
        </div>
      ) : (
        <div className="space-y-2">
          {secrets.map(secret => (
            <div key={secret.name} className="surface px-4 py-3 flex items-center justify-between">
              <div className="flex items-center gap-3 min-w-0">
                <KeyRound size={13} className="text-muted-foreground/60 shrink-0" />
                <div className="min-w-0 flex items-baseline gap-2">
                  <span className="text-[13px] font-mono">{secret.name}</span>
                  {secret.created_at && (
                    <span className="text-[10px] text-muted-foreground/60">{secret.created_at}</span>
                  )}
                </div>
              </div>
              <button
                className="p-1.5 rounded-md text-muted-foreground/60 hover:text-danger hover:bg-accent transition-colors shrink-0"
                onClick={() => handleDelete(secret.name)}
                title="Delete"
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))}
        </div>
      )}

      {secrets.length > 0 && (
        <div className="mt-6 surface px-4 py-3">
          <div className="eyebrow mb-2">Usage</div>
          <pre className="text-[11px] text-muted-foreground font-mono leading-relaxed">
{`# Inject a secret as env var into a command
mimi secret run OPENAI_API_KEY OPENAI_API_KEY -- curl -H "Authorization: Bearer $OPENAI_API_KEY" ...

# Syntax: mimi secret run <secret_name> <env_var> -- <command...>
# The decrypted value never appears in stdout`}
          </pre>
        </div>
      )}
    </div>
  )
}

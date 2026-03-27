import { useState, useEffect } from 'react'
import { getConfig, saveConfig } from '../hooks/useApi'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Badge } from '@/components/ui/badge'

export function ConfigPanel() {
  const [json, setJson] = useState('')
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getConfig().then(c => setJson(JSON.stringify(c, null, 2))).catch(() => {})
  }, [])

  const handleSave = async () => {
    try {
      const parsed = JSON.parse(json)
      await saveConfig(parsed)
      setSaved(true)
      setError(null)
      setTimeout(() => setSaved(false), 2000)
    } catch {
      setError('Invalid JSON')
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">Configuration</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Textarea
          className="font-mono text-sm min-h-[200px]"
          value={json}
          onChange={e => { setJson(e.target.value); setSaved(false); setError(null); }}
        />
        <div className="flex items-center gap-3">
          <Button onClick={handleSave}>Save Config</Button>
          {saved && <Badge variant="default" className="bg-emerald-600">Saved</Badge>}
          {error && <Badge variant="destructive">{error}</Badge>}
        </div>
        <p className="text-xs text-muted-foreground">
          Changes to session_name or model require a relaunch to take effect.
        </p>
      </CardContent>
    </Card>
  )
}

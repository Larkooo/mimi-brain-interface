import { useState } from 'react'
import { runQuery } from '../hooks/useApi'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

const QUICK_QUERIES = [
  'SELECT * FROM entities ORDER BY updated_at DESC LIMIT 20',
  'SELECT r.*, s.name as source, t.name as target FROM relationships r JOIN entities s ON r.source_id=s.id JOIN entities t ON r.target_id=t.id',
  'SELECT type, COUNT(*) as count FROM entities GROUP BY type',
  'SELECT type, COUNT(*) as count FROM relationships GROUP BY type',
]

export function QueryPanel() {
  const [sql, setSql] = useState('SELECT * FROM entities LIMIT 20')
  const [results, setResults] = useState<[string, string][][] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const execute = async () => {
    try {
      setError(null)
      const rows = await runQuery(sql)
      setResults(rows)
    } catch (e) {
      setError((e as Error).message)
      setResults(null)
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">SQL Query</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <Textarea
            className="font-mono text-sm"
            value={sql}
            onChange={e => setSql(e.target.value)}
            onKeyDown={e => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') execute() }}
            placeholder="SELECT * FROM entities LIMIT 20"
            rows={4}
          />
          <div className="flex items-center gap-3">
            <Button onClick={execute}>Run Query</Button>
            <span className="text-xs text-muted-foreground">Cmd+Enter</span>
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}

          {results !== null && (
            results.length === 0 ? (
              <p className="text-sm text-muted-foreground text-center py-4">(no results)</p>
            ) : (
              <div className="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      {results[0].map(([col]) => <TableHead key={col}>{col}</TableHead>)}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {results.map((row, i) => (
                      <TableRow key={i}>
                        {row.map(([col, val]) => <TableCell key={col} className="font-mono text-xs">{val}</TableCell>)}
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Quick Queries</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-1">
            {QUICK_QUERIES.map(q => (
              <button
                key={q}
                className="block w-full text-left text-xs font-mono text-muted-foreground hover:text-foreground py-1 px-2 rounded hover:bg-muted transition-colors"
                onClick={() => setSql(q)}
              >
                {q}
              </button>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  )
}

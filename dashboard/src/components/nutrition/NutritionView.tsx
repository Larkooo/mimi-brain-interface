import { useEffect, useMemo, useState } from 'react'
import {
  getNutritionToday,
  getNutritionWeek,
  getNutritionGoals,
  setNutritionGoals,
  logNutrition,
  deleteNutritionLog,
} from '../../hooks/useApi'
import type {
  NutritionDay,
  NutritionTrend,
  NutritionGoals,
} from '../../hooks/useApi'
import { Flame, Beef, Wheat, Droplet, Trash2, Plus, Target } from 'lucide-react'

const ACCENT = 'oklch(0.78 0.14 268)'  // var(--brand)
const ACCENT_FG = 'oklch(0.135 0.004 280)' // dark fg for accent buttons
const SURFACE_BG = 'color-mix(in oklch, var(--card) 60%, transparent)'
const SURFACE_BORDER = 'var(--border)'
const CELL_BG = 'color-mix(in oklch, var(--muted) 80%, transparent)'

export function NutritionView() {
  const [today, setToday] = useState<NutritionDay | null>(null)
  const [trend, setTrend] = useState<NutritionTrend | null>(null)
  const [goals, setGoals] = useState<NutritionGoals | null>(null)
  const [showGoals, setShowGoals] = useState(false)
  const [busy, setBusy] = useState(false)

  const refresh = async () => {
    try {
      const [t, w, g] = await Promise.all([
        getNutritionToday(),
        getNutritionWeek(),
        getNutritionGoals().catch(() => null),
      ])
      setToday(t)
      setTrend(w)
      setGoals(g)
    } catch {}
  }

  useEffect(() => {
    refresh()
    const t = setInterval(refresh, 15000)
    return () => clearInterval(t)
  }, [])

  const handleDelete = async (id: number) => {
    setBusy(true)
    try { await deleteNutritionLog(id); await refresh() } finally { setBusy(false) }
  }

  return (
    <div className="px-10 py-10 max-w-5xl mx-auto">
      <header className="mb-8">
        <div className="eyebrow mb-1.5">Section</div>
        <div className="flex items-baseline justify-between">
          <div>
            <h1 className="text-[26px] font-semibold tracking-tight leading-none">Nutrition</h1>
            <p className="text-sm text-muted-foreground mt-2">
              {goals?.phase ?? 'no phase set'} · {goals?.weight_kg ?? '–'} kg · {goals?.bodyfat_pct ?? '–'}% BF
            </p>
          </div>
          <button
            onClick={() => setShowGoals(v => !v)}
            className="text-[12px] text-muted-foreground hover:text-foreground flex items-center gap-1.5 transition-colors"
          >
            <Target size={14} /> {showGoals ? 'hide goals' : 'edit goals'}
          </button>
        </div>
      </header>

      {showGoals && goals && (
        <GoalsEditor
          goals={goals}
          onSave={async (g) => { await setNutritionGoals(g); await refresh(); setShowGoals(false) }}
        />
      )}

      <TodaySection today={today} goals={goals} />

      <QuickLog onLogged={refresh} />

      <MealsList meals={today?.meals ?? []} onDelete={handleDelete} busy={busy} />

      <WeekChart trend={trend} goals={goals} />
    </div>
  )
}

function TodaySection({ today, goals }: { today: NutritionDay | null; goals: NutritionGoals | null }) {
  const totals = today?.totals ?? { calories: 0, protein_g: 0, carbs_g: 0, fat_g: 0, meals_count: 0 }
  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
      <MacroCard
        icon={<Flame size={16} />}
        label="calories"
        value={totals.calories}
        goal={goals?.target_cals ?? null}
        unit="cal"
      />
      <MacroCard
        icon={<Beef size={16} />}
        label="protein"
        value={totals.protein_g}
        goal={goals?.target_protein_g ?? null}
        unit="g"
      />
      <MacroCard
        icon={<Wheat size={16} />}
        label="carbs"
        value={totals.carbs_g}
        goal={goals?.target_carbs_g ?? null}
        unit="g"
      />
      <MacroCard
        icon={<Droplet size={16} />}
        label="fat"
        value={totals.fat_g}
        goal={goals?.target_fat_g ?? null}
        unit="g"
      />
    </div>
  )
}

function MacroCard({ icon, label, value, goal, unit }: {
  icon: React.ReactNode; label: string; value: number; goal: number | null; unit: string
}) {
  const pct = goal ? Math.min(100, Math.round((value / goal) * 100)) : 0
  const over = goal ? value > goal : false
  return (
    <div
      className="rounded-lg p-4"
      style={{ background: SURFACE_BG, border: `1px solid ${SURFACE_BORDER}` }}
    >
      <div className="flex items-center gap-1.5 text-xs text-muted-foreground mb-2">
        {icon}
        <span>{label}</span>
      </div>
      <div className="flex items-baseline gap-2">
        <span className="text-2xl font-semibold tabular-nums">{Math.round(value)}</span>
        <span className="text-xs text-muted-foreground/80">/ {goal ?? '–'} {unit}</span>
      </div>
      <div className="mt-3 h-1.5 rounded-full overflow-hidden" style={{ background: CELL_BG }}>
        <div
          className="h-full rounded-full transition-all duration-500"
          style={{
            width: `${Math.min(100, pct)}%`,
            background: over ? 'var(--danger)' : ACCENT,
            boxShadow: over ? '0 0 8px var(--danger)' : `0 0 8px ${ACCENT}`,
          }}
        />
      </div>
      {goal && (
        <div className="text-[10px] text-muted-foreground/80 mt-1 tabular-nums">
          {pct}% · {Math.round(Math.max(0, goal - value))} {unit} left
        </div>
      )}
    </div>
  )
}

function QuickLog({ onLogged }: { onLogged: () => void }) {
  const [food, setFood] = useState('')
  const [cal, setCal] = useState('')
  const [prot, setProt] = useState('')
  const [carbs, setCarbs] = useState('')
  const [fat, setFat] = useState('')
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    if (!food || !cal) return
    setBusy(true)
    try {
      await logNutrition({
        food_text: food,
        calories: Number(cal),
        protein_g: Number(prot || '0'),
        carbs_g: Number(carbs || '0'),
        fat_g: Number(fat || '0'),
        source: 'web',
      })
      setFood(''); setCal(''); setProt(''); setCarbs(''); setFat('')
      onLogged()
    } finally { setBusy(false) }
  }

  const cellStyle = { background: CELL_BG, border: `1px solid ${SURFACE_BORDER}` }

  return (
    <div
      className="rounded-lg p-4 mb-6"
      style={{ background: SURFACE_BG, border: `1px solid ${SURFACE_BORDER}` }}
    >
      <div className="text-xs text-muted-foreground mb-3 flex items-center gap-1.5">
        <Plus size={14} /> quick log
      </div>
      <div className="grid grid-cols-12 gap-2">
        <input
          placeholder="food..."
          value={food}
          onChange={(e) => setFood(e.target.value)}
          className="col-span-12 md:col-span-5 px-3 py-2 rounded text-sm"
          style={cellStyle}
        />
        <input
          placeholder="cal"
          type="number"
          value={cal}
          onChange={(e) => setCal(e.target.value)}
          className="col-span-3 md:col-span-1 px-2 py-2 rounded text-sm tabular-nums"
          style={cellStyle}
        />
        <input
          placeholder="prot"
          type="number"
          value={prot}
          onChange={(e) => setProt(e.target.value)}
          className="col-span-3 md:col-span-1 px-2 py-2 rounded text-sm tabular-nums"
          style={cellStyle}
        />
        <input
          placeholder="carbs"
          type="number"
          value={carbs}
          onChange={(e) => setCarbs(e.target.value)}
          className="col-span-3 md:col-span-1 px-2 py-2 rounded text-sm tabular-nums"
          style={cellStyle}
        />
        <input
          placeholder="fat"
          type="number"
          value={fat}
          onChange={(e) => setFat(e.target.value)}
          className="col-span-3 md:col-span-1 px-2 py-2 rounded text-sm tabular-nums"
          style={cellStyle}
        />
        <button
          onClick={submit}
          disabled={busy || !food || !cal}
          className="col-span-12 md:col-span-3 px-3 py-2 rounded text-sm font-medium transition-colors disabled:opacity-40"
          style={{ background: ACCENT, color: ACCENT_FG }}
        >
          {busy ? 'logging…' : 'log it'}
        </button>
      </div>
    </div>
  )
}

function MealsList({ meals, onDelete, busy }: {
  meals: NonNullable<NutritionDay['meals']>; onDelete: (id: number) => void; busy: boolean
}) {
  if (!meals.length) {
    return (
      <div className="text-sm text-muted-foreground/80 mb-6 text-center py-6 rounded-lg"
        style={{ background: SURFACE_BG, border: `1px dashed ${SURFACE_BORDER}` }}>
        no meals logged today
      </div>
    )
  }
  return (
    <div className="rounded-lg mb-6 overflow-hidden"
      style={{ background: SURFACE_BG, border: `1px solid ${SURFACE_BORDER}` }}>
      <div className="px-4 py-2 text-xs text-muted-foreground border-b" style={{ borderColor: SURFACE_BORDER }}>
        today · {meals.length} meals
      </div>
      {meals.map((m) => (
        <div key={m.id} className="px-4 py-2.5 flex items-center gap-3 text-sm border-b last:border-b-0"
          style={{ borderColor: SURFACE_BORDER }}>
          <div className="flex-1 min-w-0">
            <div className="truncate">{m.food_text}</div>
            <div className="text-[10px] text-muted-foreground/80 tabular-nums">
              {m.logged_at.split(' ')[1] ?? m.logged_at} · {m.source}
            </div>
          </div>
          <div className="text-right tabular-nums text-foreground w-16">
            {Math.round(m.calories)}<span className="text-muted-foreground/80 text-xs"> cal</span>
          </div>
          <div className="text-right tabular-nums text-foreground/80 w-14">
            {Math.round(m.protein_g)}<span className="text-muted-foreground/80 text-xs">p</span>
          </div>
          <button
            onClick={() => onDelete(m.id)}
            disabled={busy}
            className="text-muted-foreground/70 hover:text-red-400 transition-colors disabled:opacity-40"
            title="delete"
          >
            <Trash2 size={14} />
          </button>
        </div>
      ))}
    </div>
  )
}

function WeekChart({ trend, goals }: { trend: NutritionTrend | null; goals: NutritionGoals | null }) {
  const days = trend?.days ?? []
  const goal = goals?.target_cals ?? null

  const { bars, maxCal } = useMemo(() => {
    const maxCal = Math.max(
      goal ?? 0,
      ...days.map((d) => d.cal),
      1
    )
    const today = new Date().toISOString().slice(0, 10)
    const range: { date: string; cal: number; prot: number; isToday: boolean; empty: boolean }[] = []
    for (let i = 6; i >= 0; i--) {
      const d = new Date()
      d.setDate(d.getDate() - i)
      const key = d.toISOString().slice(0, 10)
      const row = days.find((x) => x.date === key)
      range.push({
        date: key,
        cal: row?.cal ?? 0,
        prot: row?.prot ?? 0,
        isToday: key === today,
        empty: !row,
      })
    }
    return { bars: range, maxCal }
  }, [days, goal])

  return (
    <div className="rounded-lg p-4"
      style={{ background: SURFACE_BG, border: `1px solid ${SURFACE_BORDER}` }}>
      <div className="flex items-baseline justify-between mb-4">
        <div className="text-xs text-muted-foreground">last 7 days</div>
        <div className="text-xs text-muted-foreground/80 tabular-nums">
          avg {Math.round(trend?.avg.calories ?? 0)} cal · {Math.round(trend?.avg.protein_g ?? 0)}g prot
        </div>
      </div>
      <div className="flex items-end gap-2 h-40 mb-2">
        {bars.map((b) => {
          const h = maxCal ? (b.cal / maxCal) * 100 : 0
          const goalH = goal ? (goal / maxCal) * 100 : 0
          const over = goal ? b.cal > goal * 1.05 : false
          const under = goal ? b.cal < goal * 0.85 : false
          const color = b.empty ? 'rgba(255,255,255,0.08)'
            : over ? '#ff6b6b'
            : under ? '#ffd36b'
            : ACCENT
          return (
            <div key={b.date} className="flex-1 flex flex-col items-center gap-1 relative">
              <div className="w-full relative" style={{ height: '140px' }}>
                {goal && (
                  <div
                    className="absolute left-0 right-0 border-t border-dashed"
                    style={{
                      bottom: `${goalH}%`,
                      borderColor: SURFACE_BORDER,
                    }}
                  />
                )}
                <div
                  className="absolute bottom-0 left-0 right-0 rounded-t transition-all duration-500"
                  style={{
                    height: `${h}%`,
                    background: color,
                    opacity: b.empty ? 0.3 : 0.85,
                    boxShadow: b.isToday ? `0 0 8px ${color}` : undefined,
                  }}
                />
              </div>
              <div className="text-[10px] text-muted-foreground tabular-nums">
                {b.cal > 0 ? Math.round(b.cal) : '–'}
              </div>
              <div className={`text-[10px] ${b.isToday ? 'text-foreground' : 'text-muted-foreground/80'}`}>
                {new Date(b.date).toLocaleDateString(undefined, { weekday: 'short' })}
              </div>
            </div>
          )
        })}
      </div>
      {goal && (
        <div className="text-[10px] text-muted-foreground/70 text-center">
          dashed line = target {goal} cal
        </div>
      )}
    </div>
  )
}

function GoalsEditor({ goals, onSave }: {
  goals: NutritionGoals; onSave: (g: Partial<NutritionGoals>) => Promise<void>
}) {
  const [form, setForm] = useState({
    target_cals: goals.target_cals ?? 2800,
    target_protein_g: goals.target_protein_g ?? 145,
    target_carbs_g: goals.target_carbs_g ?? 350,
    target_fat_g: goals.target_fat_g ?? 78,
    tdee: goals.tdee ?? 2500,
    weight_kg: goals.weight_kg ?? 66,
    height_cm: goals.height_cm ?? 175,
    bodyfat_pct: goals.bodyfat_pct ?? 14,
    phase: goals.phase ?? 'lean_bulk',
  })
  const [busy, setBusy] = useState(false)
  const cellStyle = { background: CELL_BG, border: `1px solid ${SURFACE_BORDER}` }

  const field = (k: keyof typeof form, label: string, type: 'number' | 'text' = 'number') => (
    <label className="text-xs text-muted-foreground flex flex-col gap-1">
      {label}
      <input
        type={type}
        value={String(form[k] ?? '')}
        onChange={(e) => setForm({ ...form, [k]: type === 'number' ? Number(e.target.value) : e.target.value })}
        className="px-2 py-1.5 rounded text-sm text-foreground tabular-nums"
        style={cellStyle}
      />
    </label>
  )

  return (
    <div className="rounded-lg p-4 mb-6"
      style={{ background: SURFACE_BG, border: `1px solid ${SURFACE_BORDER}` }}>
      <div className="text-xs text-muted-foreground mb-3">goals</div>
      <div className="grid grid-cols-3 md:grid-cols-5 gap-3 mb-3">
        {field('target_cals', 'cal target')}
        {field('target_protein_g', 'prot (g)')}
        {field('target_carbs_g', 'carbs (g)')}
        {field('target_fat_g', 'fat (g)')}
        {field('tdee', 'tdee')}
        {field('weight_kg', 'weight (kg)')}
        {field('height_cm', 'height (cm)')}
        {field('bodyfat_pct', 'bf %')}
        {field('phase', 'phase', 'text')}
      </div>
      <button
        onClick={async () => { setBusy(true); try { await onSave(form) } finally { setBusy(false) } }}
        disabled={busy}
        className="px-3 py-1.5 rounded text-sm disabled:opacity-40"
        style={{ background: ACCENT, color: ACCENT_FG }}
      >
        {busy ? 'saving…' : 'save goals'}
      </button>
    </div>
  )
}

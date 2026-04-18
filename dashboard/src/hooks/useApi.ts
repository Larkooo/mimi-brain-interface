import { useState, useEffect, useCallback } from 'react';

export interface BrainStats {
  entities: number;
  relationships: number;
  memory_refs: number;
  entity_types: [string, number][];
  relationship_types: [string, number][];
}

export interface Entity {
  id: number;
  type: string;
  name: string;
  properties: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface Channel {
  name: string;
  type: string;
  plugin: string;
  enabled: boolean;
}

export interface Status {
  name: string;
  session_name: string;
  model: string;
  dashboard_port: number;
  session_running: boolean;
  claude_version: string;
  brain_stats: BrainStats;
  memory_files: number;
  channels: Channel[];
}

async function api<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export function useStatus() {
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api<Status>('/api/status'));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 8000);
    return () => clearInterval(interval);
  }, [refresh]);

  return { status, error, refresh };
}

export async function launchSession() {
  return api('/api/session/launch', { method: 'POST' });
}

export async function stopSession() {
  return api('/api/session/stop', { method: 'POST' });
}

export async function getEntities(type?: string): Promise<Entity[]> {
  const params = type ? `?type=${encodeURIComponent(type)}` : '';
  return api(`/api/brain/entities${params}`);
}

export async function deleteEntity(id: number) {
  return api(`/api/brain/entities/${id}`, { method: 'DELETE' });
}

export async function searchEntities(q: string): Promise<Entity[]> {
  return api(`/api/brain/search?q=${encodeURIComponent(q)}`);
}

export async function addEntity(type: string, name: string, properties: string) {
  return api('/api/brain/entities/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type, name, properties }),
  });
}

export async function addRelationship(sourceId: number, type: string, targetId: number) {
  return api('/api/brain/relationships/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ source_id: sourceId, target_id: targetId, type }),
  });
}

export async function runQuery(sql: string): Promise<[string, string][][]> {
  return api('/api/brain/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ sql }),
  });
}

export async function getConfig(): Promise<Record<string, unknown>> {
  return api('/api/config');
}

export async function saveConfig(config: Record<string, unknown>) {
  return api('/api/config', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
}

export async function addChannel(type: string, token?: string) {
  return api('/api/channels/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type, token }),
  });
}

export async function removeChannel(name: string) {
  return api(`/api/channels/${name}`, { method: 'DELETE' });
}

export async function toggleChannel(name: string) {
  return api(`/api/channels/${name}/toggle`, { method: 'POST' });
}

export async function configureChannel(name: string, token: string) {
  return api(`/api/channels/${name}/configure`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ token }),
  });
}

export async function createBackup() {
  return api('/api/backup', { method: 'POST' });
}

// --- Brain Graph ---

export interface GraphNode {
  id: number;
  name: string;
  type: string;
  properties: Record<string, unknown>;
  connections: number;
}

export interface GraphLink {
  source: number;
  target: number;
  type: string;
}

export interface GraphData {
  nodes: GraphNode[];
  links: GraphLink[];
}

export async function getGraph(): Promise<GraphData> {
  return api('/api/brain/graph');
}

// --- Crons ---

export interface CronJob {
  id: string;
  name: string;
  schedule: string;
  prompt: string;
  description: string;
  enabled: boolean;
}

export async function getCrons(): Promise<CronJob[]> {
  return api('/api/crons');
}

export async function createCron(name: string, schedule: string, prompt: string, description: string) {
  return api('/api/crons', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, schedule, prompt, description }),
  });
}

export async function deleteCron(id: string) {
  return api(`/api/crons/${id}`, { method: 'DELETE' });
}

export async function toggleCron(id: string) {
  return api(`/api/crons/${id}/toggle`, { method: 'POST' });
}

// --- Secrets ---

export interface SecretEntry {
  name: string;
  created_at: string;
}

export async function getSecrets(): Promise<SecretEntry[]> {
  return api('/api/secrets');
}

export async function setSecret(name: string, value: string) {
  return api('/api/secrets', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, value }),
  });
}

export async function deleteSecret(name: string) {
  return api(`/api/secrets/${name}`, { method: 'DELETE' });
}

// --- Memory ---

export interface MemoryFile {
  name: string;
  description: string;
  type: string;
  filename: string;
}

export function useMemoryFiles() {
  const [files, setFiles] = useState<MemoryFile[]>([]);

  useEffect(() => {
    api<MemoryFile[]>('/api/memory')
      .then(setFiles)
      .catch(() => setFiles([]));
  }, []);

  return { files };
}

export async function getMemoryFile(filename: string): Promise<{ content: string }> {
  return api(`/api/memory/${encodeURIComponent(filename)}`);
}

// Logs
export interface LogEntry { name: string; path: string; size: number; exists: boolean }
export async function getLogs(): Promise<LogEntry[]> { return api('/api/logs'); }
export async function tailLog(name: string): Promise<{ name: string; path: string; text: string; lines: number }> {
  return api(`/api/logs/${encodeURIComponent(name)}`);
}

// Services
export interface ServiceInfo {
  name: string;
  active_state: string;
  sub_state: string;
  main_pid: number | null;
  enabled: boolean;
}
export async function getServices(): Promise<ServiceInfo[]> { return api('/api/services'); }
export async function restartService(name: string) {
  return api(`/api/services/${encodeURIComponent(name)}/restart`, { method: 'POST' });
}
export async function startService(name: string) {
  return api(`/api/services/${encodeURIComponent(name)}/start`, { method: 'POST' });
}
export async function stopService(name: string) {
  return api(`/api/services/${encodeURIComponent(name)}/stop`, { method: 'POST' });
}

// --- Nutrition ---

export interface NutritionMeal {
  id: number;
  meal_date: string;
  logged_at: string;
  food_text: string;
  calories: number;
  protein_g: number;
  carbs_g: number;
  fat_g: number;
  source: string;
}

export interface NutritionTotals {
  calories: number;
  protein_g: number;
  carbs_g: number;
  fat_g: number;
  meals_count: number;
}

export interface NutritionDay {
  date: string;
  user: string;
  totals: NutritionTotals;
  meals: NutritionMeal[];
}

export interface NutritionDayRow {
  date: string;
  cal: number;
  prot: number;
  carbs: number;
  fat: number;
  meals: number;
}

export interface NutritionTrend {
  days: NutritionDayRow[];
  avg: NutritionTotals;
}

export interface NutritionGoals {
  user: string;
  tdee: number | null;
  target_cals: number | null;
  target_protein_g: number | null;
  target_carbs_g: number | null;
  target_fat_g: number | null;
  weight_kg: number | null;
  height_cm: number | null;
  bodyfat_pct: number | null;
  phase: string | null;
  updated_at: string | null;
}

export async function getNutritionToday(): Promise<NutritionDay> {
  return api('/api/nutrition/today');
}
export async function getNutritionDay(date: string): Promise<NutritionDay> {
  return api(`/api/nutrition/day/${date}`);
}
export async function getNutritionWeek(): Promise<NutritionTrend> {
  return api('/api/nutrition/week');
}
export async function getNutritionMonth(): Promise<NutritionTrend> {
  return api('/api/nutrition/month');
}
export async function getNutritionGoals(): Promise<NutritionGoals> {
  return api('/api/nutrition/goals');
}
export async function setNutritionGoals(goals: Partial<NutritionGoals>) {
  return api('/api/nutrition/goals', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(goals),
  });
}
export async function logNutrition(meal: {
  food_text: string;
  calories: number;
  protein_g?: number;
  carbs_g?: number;
  fat_g?: number;
  meal_date?: string;
  source?: string;
}) {
  return api('/api/nutrition/log', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(meal),
  });
}
export async function deleteNutritionLog(id: number) {
  return api(`/api/nutrition/log/${id}`, { method: 'DELETE' });
}

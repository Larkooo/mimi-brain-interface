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

export async function deleteEntity(id: number) {
  return api(`/api/brain/entities/${id}`, { method: 'DELETE' });
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

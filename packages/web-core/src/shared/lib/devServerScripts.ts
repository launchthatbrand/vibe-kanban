import type { Repo } from 'shared/types';

export interface DevServerScriptEntry {
  id: string;
  name: string;
  script: string;
}

interface StoredDevServerScriptsV1 {
  version?: number;
  scripts?: Array<{
    id?: string;
    name?: string;
    script?: string;
  }>;
  entries?: Array<{
    id?: string;
    name?: string;
    script?: string;
  }>;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null;

const sanitizeId = (value: string): string =>
  value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-_]+/g, '-')
    .replace(/^-+|-+$/g, '');

const createStableId = (name: string, index: number): string => {
  const preferred = sanitizeId(name);
  if (preferred) return preferred;
  return `script-${index + 1}`;
};

const normalizeEntries = (
  rawEntries: Array<{ id?: string; name?: string; script?: string }>
): DevServerScriptEntry[] => {
  const seen = new Set<string>();
  const entries: DevServerScriptEntry[] = [];

  rawEntries.forEach((raw, index) => {
    const script = (raw.script ?? '').trim();
    if (!script) return;

    const baseName = (raw.name ?? '').trim() || `Script ${index + 1}`;
    const requestedId = sanitizeId(raw.id ?? '');
    const fallbackId = createStableId(baseName, index);
    let id = requestedId || fallbackId;

    if (seen.has(id)) {
      let suffix = 2;
      while (seen.has(`${id}-${suffix}`)) {
        suffix += 1;
      }
      id = `${id}-${suffix}`;
    }

    seen.add(id);
    entries.push({
      id,
      name: baseName,
      script,
    });
  });

  return entries;
};

export const parseDevServerScripts = (
  rawValue: string | null | undefined
): DevServerScriptEntry[] => {
  const value = (rawValue ?? '').trim();
  if (!value) return [];

  try {
    const parsed = JSON.parse(value) as unknown;

    if (Array.isArray(parsed)) {
      return normalizeEntries(
        parsed as Array<{ id?: string; name?: string; script?: string }>
      );
    }

    if (isRecord(parsed)) {
      const maybeTyped = parsed as StoredDevServerScriptsV1;
      const scripts = maybeTyped.scripts ?? maybeTyped.entries;
      if (Array.isArray(scripts)) {
        return normalizeEntries(scripts);
      }

      const singleScript = parsed.script;
      if (typeof singleScript === 'string' && singleScript.trim()) {
        return [
          {
            id: 'default',
            name: 'Default',
            script: singleScript.trim(),
          },
        ];
      }
    }
  } catch {
    // Legacy single script value.
  }

  return [
    {
      id: 'default',
      name: 'Default',
      script: value,
    },
  ];
};

export const stringifyDevServerScripts = (
  entries: DevServerScriptEntry[]
): string | null => {
  const normalized = normalizeEntries(entries);
  if (normalized.length === 0) return null;

  if (
    normalized.length === 1 &&
    normalized[0].id === 'default' &&
    normalized[0].name.trim().toLowerCase() === 'default'
  ) {
    return normalized[0].script;
  }

  return JSON.stringify(
    {
      version: 1,
      scripts: normalized.map((entry) => ({
        id: entry.id,
        name: entry.name,
        script: entry.script,
      })),
    },
    null,
    2
  );
};

export const getRepoDevServerScripts = (
  repo: Pick<Repo, 'dev_server_script'>
): DevServerScriptEntry[] => parseDevServerScripts(repo.dev_server_script);

#!/usr/bin/env node

const DEFAULT_OLD_BASE_URL = 'http://127.0.0.1:3456/api';
const DEFAULT_NEW_BASE_URL = 'http://127.0.0.1:3457/api';
const SAMPLE_LIMIT = Number.parseInt(process.env.COMPARE_SAMPLE_LIMIT ?? '25', 10);
const COMPARE_VALUES = process.env.COMPARE_VALUES === '1';
const VALUE_SUMMARY_FIELDS = [
  'inputTokens',
  'outputTokens',
  'cacheCreationTokens',
  'cacheReadTokens',
  'totalTokens',
  'totalCost',
  'cost',
];

const SOURCES = ['claude', 'codex', 'opencode', 'hermes', 'openclaw', 'grok'];
const VIEWS = ['daily', 'monthly', 'sessions', 'blocks'];
const DEFAULT_ENDPOINTS = [
  'health',
  ...SOURCES.flatMap((source) => VIEWS.map((view) => `${source}/${view}`)),
];

const oldBaseUrl = normalizeBaseUrl(process.env.OLD_BASE_URL ?? DEFAULT_OLD_BASE_URL);
const newBaseUrl = normalizeBaseUrl(process.env.NEW_BASE_URL ?? DEFAULT_NEW_BASE_URL);
const endpoints = parseEndpoints(process.env.COMPARE_ENDPOINTS) ?? DEFAULT_ENDPOINTS;

function normalizeBaseUrl(baseUrl) {
  return baseUrl.replace(/\/+$/, '');
}

function parseEndpoints(value) {
  if (!value) return null;
  const parsed = value
    .split(',')
    .map((endpoint) => endpoint.trim().replace(/^\/+/, ''))
    .filter(Boolean);
  return parsed.length > 0 ? parsed : null;
}

function endpointUrl(baseUrl, endpoint) {
  return `${baseUrl}/${endpoint}`;
}

async function fetchJson(baseUrl, endpoint) {
  const url = endpointUrl(baseUrl, endpoint);

  try {
    const response = await fetch(url, {
      headers: { accept: 'application/json' },
      signal: AbortSignal.timeout(30_000),
    });
    const text = await response.text();
    let json = null;
    let parseError = null;

    if (text.length > 0) {
      try {
        json = JSON.parse(text);
      } catch (error) {
        parseError = error.message;
      }
    }

    return {
      url,
      status: response.status,
      ok: response.ok,
      json,
      parseError,
      textLength: text.length,
    };
  } catch (error) {
    return {
      url,
      status: null,
      ok: false,
      json: null,
      parseError: null,
      fetchError: error.message,
      textLength: 0,
    };
  }
}

function typeName(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  return typeof value;
}

function addType(schema, path, type) {
  if (!schema.has(path)) {
    schema.set(path, new Set());
  }
  schema.get(path).add(type);
}

function collectSchema(value, schema = new Map(), path = '$') {
  const type = typeName(value);
  addType(schema, path, type);

  if (Array.isArray(value)) {
    const sample = value.slice(0, Number.isFinite(SAMPLE_LIMIT) && SAMPLE_LIMIT > 0 ? SAMPLE_LIMIT : 25);
    if (sample.length === 0) {
      addType(schema, `${path}[]`, 'empty');
      return schema;
    }
    for (const item of sample) {
      collectSchema(item, schema, `${path}[]`);
    }
    return schema;
  }

  if (value && typeof value === 'object') {
    for (const key of Object.keys(value).sort()) {
      collectSchema(value[key], schema, `${path}.${key}`);
    }
  }

  return schema;
}

function sortedSchemaEntries(schema) {
  return Array.from(schema.entries())
    .map(([path, types]) => [path, Array.from(types).sort()])
    .sort(([left], [right]) => left.localeCompare(right));
}

function compareSchemas(oldJson, newJson) {
  const oldSchema = collectSchema(oldJson);
  const newSchema = collectSchema(newJson);
  const oldPaths = new Set(oldSchema.keys());
  const newPaths = new Set(newSchema.keys());
  const diffs = [];

  for (const [path, oldTypes] of sortedSchemaEntries(oldSchema)) {
    if (!newPaths.has(path)) {
      diffs.push(`missing in new: ${path} (${oldTypes.join('|')})`);
      continue;
    }

    const newTypes = Array.from(newSchema.get(path)).sort();
    if (oldTypes.join('|') !== newTypes.join('|')) {
      diffs.push(`type mismatch: ${path} old=${oldTypes.join('|')} new=${newTypes.join('|')}`);
    }
  }

  for (const [path, newTypes] of sortedSchemaEntries(newSchema)) {
    if (!oldPaths.has(path)) {
      diffs.push(`missing in old: ${path} (${newTypes.join('|')})`);
    }
  }

  return diffs;
}

function summarizeArrayValues(items) {
  const summary = {
    length: items.length,
  };

  for (const field of VALUE_SUMMARY_FIELDS) {
    summary[field] = 0;
  }

  for (const item of items) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) {
      continue;
    }

    for (const field of VALUE_SUMMARY_FIELDS) {
      const value = item[field];
      if (typeof value === 'number' && Number.isFinite(value)) {
        summary[field] += value;
      }
    }
  }

  return summary;
}

function compareArrayValues(oldJson, newJson) {
  if (!Array.isArray(oldJson) || !Array.isArray(newJson)) {
    return [];
  }

  const oldSummary = summarizeArrayValues(oldJson);
  const newSummary = summarizeArrayValues(newJson);
  const diffs = [];

  if (oldSummary.length !== newSummary.length) {
    diffs.push(`array length mismatch: old=${oldSummary.length} new=${newSummary.length}`);
  }

  for (const field of VALUE_SUMMARY_FIELDS) {
    if (oldSummary[field] !== newSummary[field]) {
      diffs.push(`${field} mismatch: old=${oldSummary[field]} new=${newSummary[field]}`);
    }
  }

  return diffs;
}

function compareResponses(endpoint, oldResponse, newResponse) {
  const diffs = [];

  if (oldResponse.fetchError) {
    diffs.push(`old fetch failed: ${oldResponse.fetchError}`);
  }
  if (newResponse.fetchError) {
    diffs.push(`new fetch failed: ${newResponse.fetchError}`);
  }
  if (oldResponse.status !== newResponse.status) {
    diffs.push(`status mismatch: old=${oldResponse.status ?? 'none'} new=${newResponse.status ?? 'none'}`);
  }
  if (oldResponse.parseError) {
    diffs.push(`old JSON parse failed: ${oldResponse.parseError}`);
  }
  if (newResponse.parseError) {
    diffs.push(`new JSON parse failed: ${newResponse.parseError}`);
  }

  if (!oldResponse.fetchError && !newResponse.fetchError && !oldResponse.parseError && !newResponse.parseError) {
    diffs.push(...compareSchemas(oldResponse.json, newResponse.json));
    if (COMPARE_VALUES) {
      diffs.push(...compareArrayValues(oldResponse.json, newResponse.json));
    }
  }

  return {
    endpoint,
    oldUrl: oldResponse.url,
    newUrl: newResponse.url,
    diffs,
  };
}

async function compareEndpoint(endpoint) {
  const [oldResponse, newResponse] = await Promise.all([
    fetchJson(oldBaseUrl, endpoint),
    fetchJson(newBaseUrl, endpoint),
  ]);
  return compareResponses(endpoint, oldResponse, newResponse);
}

function printResult(result) {
  if (result.diffs.length === 0) {
    console.log(`ok   /${result.endpoint}`);
    return;
  }

  console.log(`diff /${result.endpoint}`);
  for (const diff of result.diffs.slice(0, 12)) {
    console.log(`  - ${diff}`);
  }
  if (result.diffs.length > 12) {
    console.log(`  - ... ${result.diffs.length - 12} more difference(s)`);
  }
}

async function main() {
  console.log(`old: ${oldBaseUrl}`);
  console.log(`new: ${newBaseUrl}`);
  console.log(`endpoints: ${endpoints.length}`);

  const results = [];
  for (const endpoint of endpoints) {
    const result = await compareEndpoint(endpoint);
    results.push(result);
    printResult(result);
  }

  const failed = results.filter((result) => result.diffs.length > 0);
  if (failed.length > 0) {
    const diffCount = failed.reduce((sum, result) => sum + result.diffs.length, 0);
    console.error(`\n${failed.length} endpoint(s) differ, ${diffCount} total difference(s).`);
    process.exitCode = 1;
    return;
  }

  console.log('\nAll compared endpoint structures match.');
}

await main();

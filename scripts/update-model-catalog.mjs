import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const sourceUrl = "https://api.github.com/repos/openai/codex/contents/codex-rs/models-manager/models.json";
const catalogPath = fileURLToPath(new URL("../model-catalog/catalog.json", import.meta.url));

export function updateCatalog(current, upstream, version, models = []) {
  if (!version?.trim() || version === current.version) {
    throw new Error("Provide a new, non-empty release --version");
  }
  if (!Array.isArray(upstream.models) || upstream.models.length === 0) {
    throw new Error("Official catalog must contain a non-empty models array");
  }
  const imported = new Map();
  for (const original of upstream.models) {
    const row = structuredClone(original);
    if (typeof row.slug !== "string" || !row.slug.trim()) {
      throw new Error("Official model is missing a slug");
    }
    const key = row.slug.toLowerCase();
    if (imported.has(key)) throw new Error(`Duplicate official model: ${row.slug}`);
    // Current clients require this legacy field. Preserve every official field,
    // and reuse the model's own prompt when upstream only provides model_messages.
    if (row.base_instructions === undefined) {
      row.base_instructions = row.model_messages?.instructions_template;
    }
    for (const field of ["display_name", "base_instructions", "shell_type", "default_reasoning_level"]) {
      if (typeof row[field] !== "string" || !row[field].trim()) {
        throw new Error(`${row.slug} is missing ${field}`);
      }
    }
    if (!Number.isSafeInteger(row.context_window) || row.context_window <= 0) {
      throw new Error(`${row.slug} has an invalid context_window`);
    }
    for (const field of ["input_modalities", "supported_reasoning_levels"]) {
      if (!Array.isArray(row[field]) || row[field].length === 0) {
        throw new Error(`${row.slug} is missing ${field}`);
      }
    }
    imported.set(key, row);
  }
  const selected = models.length ? models.map((model) => model.toLowerCase()) : [...imported.keys()];
  for (const model of selected) {
    if (!imported.has(model)) throw new Error(`Official catalog does not contain ${model}`);
  }
  // Preserve older models absent from upstream, DeepSeek profiles and the fallback.
  const rows = new Map(current.openai.map((row) => [row.slug.toLowerCase(), row]));
  for (const model of selected) rows.set(model, imported.get(model));
  const result = { ...current, version, openai: [...rows.values()] };
  const ids = new Set();
  for (const row of [...result.openai, ...result.deepseek, result.default]) {
    const key = row.slug.toLowerCase();
    if (ids.has(key)) throw new Error(`Duplicate catalog model: ${row.slug}`);
    ids.add(key);
  }
  return result;
}

async function main() {
  const { values } = parseArgs({ options: {
    version: { type: "string" },
    source: { type: "string" },
    model: { type: "string", multiple: true },
  } });
  if (!values.version?.trim()) throw new Error("Usage: pnpm catalog:update --version YYYY-MM-DD.N [--model gpt-6.1-sol] [--source models.json]");
  const current = JSON.parse(await fs.readFile(catalogPath, "utf8"));
  let upstream;
  if (values.source) {
    upstream = JSON.parse(await fs.readFile(values.source, "utf8"));
  } else {
    const response = await fetch(sourceUrl, { signal: AbortSignal.timeout(30_000) });
    if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
    const body = await response.json();
    if (body.encoding !== "base64" || !body.content) throw new Error("GitHub did not return models.json content");
    upstream = JSON.parse(Buffer.from(body.content, "base64").toString("utf8"));
    console.log(`Official models.json blob: ${body.sha}`);
  }
  const updated = updateCatalog(current, upstream, values.version, values.model);
  await fs.writeFile(catalogPath, `${JSON.stringify(updated, null, 2)}\n`);
  console.log(`Updated ${catalogPath}: ${updated.version}, ${updated.openai.length} OpenAI models.`);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}

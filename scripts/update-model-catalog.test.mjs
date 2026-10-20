import assert from "node:assert/strict";
import { test } from "node:test";
import { updateCatalog } from "./update-model-catalog.mjs";

const row = {
  slug: "gpt-6.1-sol", display_name: "GPT-6.1-Sol", shell_type: "shell_command",
  default_reasoning_level: "low", context_window: 272000,
  input_modalities: ["text", "image"], supported_reasoning_levels: [{ effort: "ultra" }],
  model_messages: { instructions_template: "Official model-specific prompt" },
  future_capability: { enabled: true },
};
const current = {
  schema_version: 1, version: "old", openai: [{ ...row, slug: "older-model" }],
  deepseek: [{ slug: "deepseek-v4-pro" }], default: { slug: "default" },
};

test("imports official capabilities and adds the legacy prompt without modifying the source", () => {
  const updated = updateCatalog(current, { models: [row] }, "new");
  assert.deepEqual(updated.openai[1], { ...row, base_instructions: row.model_messages.instructions_template });
  assert.equal(row.base_instructions, undefined);
  assert.deepEqual(updated.openai[0], current.openai[0]);
  assert.deepEqual(updated.deepseek, current.deepseek);
  assert.deepEqual(updated.default, current.default);
  assert.equal(updated.version, "new");
});

test("targeted updates preserve other profiles and replace matching slugs without duplicates", () => {
  const upstream = { models: [row, { ...row, slug: "older-model", context_window: 999999 }] };
  const first = updateCatalog(current, upstream, "new", ["gpt-6.1-sol"]);
  const second = updateCatalog(first, upstream, "newer", ["GPT-6.1-SOL"]);
  assert.equal(second.openai.length, 2);
  assert.equal(second.openai[0].context_window, 272000);
  assert.deepEqual(second.openai, first.openai);
});

test("retains an official legacy prompt when present", () => {
  const original = { ...row, base_instructions: "Official legacy prompt" };
  assert.deepEqual(updateCatalog(current, { models: [original] }, "new").openai[1], original);
});

test("rejects missing requested models, duplicate IDs, incomplete input and unchanged versions", () => {
  assert.throws(() => updateCatalog(current, { models: [row] }, "old"));
  assert.throws(() => updateCatalog(current, { models: [row] }, "new", ["absent"]));
  assert.throws(() => updateCatalog(current, { models: [row, row] }, "new"));
  assert.throws(() => updateCatalog(current, { models: [] }, "new"));
  assert.throws(() => updateCatalog(current, { models: [{ ...row, model_messages: {} }] }, "new"));
  assert.throws(() => updateCatalog(current, { models: [{ ...row, slug: "deepseek-v4-pro" }] }, "new"));
});

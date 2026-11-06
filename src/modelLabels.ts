type NamedProvider = { id: string; name: string };

function fusionModelParts(model: string) {
  return (
    /^xxswitch-([0-9a-f]+)\/(.+)$/.exec(model) ??
    /^xxswitch-claude-([0-9a-f]+)--(.+)$/.exec(model)
  );
}

export function requestModelLabel(model: string): string {
  return fusionModelParts(model)?.[2] ?? model;
}

function providerForModel(model: string, providers: NamedProvider[]) {
  const key = fusionModelParts(model)?.[1];
  if (!key) return undefined;
  return providers.find(
    (provider) =>
      Array.from(new TextEncoder().encode(provider.id), (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("") === key,
  );
}

export function fusionModelLabel(
  model: string,
  providers: NamedProvider[],
): string {
  const parts = fusionModelParts(model);
  if (!parts) return model;
  return `${parts[2]}（${providerForModel(model, providers)?.name ?? "供应商已移除"}）`;
}

/** Presentation only: never use this label as a grouping key or filter value. */
export function chartModelLabel(
  model: string,
  providers: NamedProvider[],
): string {
  const short = requestModelLabel(model);
  const provider = providerForModel(model, providers);
  return provider ? `${short}（${provider.name}）` : short;
}

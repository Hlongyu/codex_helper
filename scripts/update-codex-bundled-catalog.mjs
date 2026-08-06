import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const codex = process.env.CODEX_CLI_PATH?.trim() || "codex";
const versionOutput = execFileSync(codex, ["--version"], {
  encoding: "utf8",
}).trim();
const version = versionOutput.match(/v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\s*$/)?.[1];

if (!version) {
  throw new Error(`Could not parse Codex version from: ${versionOutput}`);
}

const rawCatalog = execFileSync(codex, ["debug", "models", "--bundled"], {
  encoding: "utf8",
});
const catalog = JSON.parse(rawCatalog);
const models = Array.isArray(catalog.models) ? catalog.models : [];

if (!models.some((model) => model?.slug === "gpt-5.6-sol")) {
  throw new Error("Codex bundled catalog does not contain gpt-5.6-sol");
}

const catalogDirectory = path.join(root, "src-tauri/src/catalogs");
fs.mkdirSync(catalogDirectory, { recursive: true });
fs.writeFileSync(
  path.join(catalogDirectory, "codex-models.json"),
  `${JSON.stringify(catalog)}\n`,
);

const libPath = path.join(root, "src-tauri/src/lib.rs");
const libSource = fs.readFileSync(libPath, "utf8");
const updatedLibSource = libSource.replace(
  /const EMBEDDED_CODEX_MODEL_CATALOG_VERSION: &str = "[^"]+";/,
  `const EMBEDDED_CODEX_MODEL_CATALOG_VERSION: &str = "${version}";`,
);

if (updatedLibSource === libSource && !libSource.includes(`"${version}"`)) {
  throw new Error("Could not update embedded Codex catalog version in src-tauri/src/lib.rs");
}

fs.writeFileSync(libPath, updatedLibSource);
console.log(`Embedded Codex ${version} bundled catalog (${models.length} models).`);

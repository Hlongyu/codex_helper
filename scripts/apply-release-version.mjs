import fs from "node:fs";
import path from "node:path";

const version = process.argv[2]?.trim();
const versionPattern =
  /^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

if (!versionPattern.test(version ?? "")) {
  throw new Error(
    `Expected a semantic version such as 1.2.3 or 1.2.3-beta.1, received: ${version ?? "<empty>"}`,
  );
}

const root = process.cwd();
for (const relativePath of ["package.json", "src-tauri/tauri.conf.json"]) {
  const filePath = path.join(root, relativePath);
  const config = JSON.parse(fs.readFileSync(filePath, "utf8"));
  config.version = version;
  fs.writeFileSync(filePath, `${JSON.stringify(config, null, 2)}\n`);
}

const cargoTomlPath = path.join(root, "src-tauri/Cargo.toml");
const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
const updatedCargoToml = cargoToml.replace(
  /^version\s*=\s*"[^"]+"/m,
  `version = "${version}"`,
);

if (updatedCargoToml === cargoToml) {
  throw new Error("Could not update the package version in src-tauri/Cargo.toml.");
}

fs.writeFileSync(cargoTomlPath, updatedCargoToml);

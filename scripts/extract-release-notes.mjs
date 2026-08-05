import fs from "node:fs";

const [tag, outputPath] = process.argv.slice(2);

if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(tag ?? "")) {
  throw new Error(`Expected a version tag such as v1.2.3, received: ${tag ?? "<empty>"}`);
}

const changelog = fs.readFileSync("CHANGELOG.md", "utf8");
const sections = [...changelog.matchAll(/^##\s+(v\S+)(?:\s+-[^\n]*)?\s*$/gm)];
const sectionIndex = sections.findIndex((match) => match[1] === tag);

if (sectionIndex === -1) {
  throw new Error(`CHANGELOG.md does not contain a section for ${tag}.`);
}

const section = sections[sectionIndex];
const start = section.index + section[0].length;
const end = sections[sectionIndex + 1]?.index ?? changelog.length;
const notes = changelog.slice(start, end).trim();

if (!notes) {
  throw new Error(`The ${tag} changelog section is empty.`);
}

if (outputPath) {
  fs.writeFileSync(outputPath, `${notes}\n`);
} else {
  process.stdout.write(`${notes}\n`);
}

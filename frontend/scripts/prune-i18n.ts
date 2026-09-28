// ============================================================
// scripts/prune-i18n.ts
// One-shot: remove the given stale keys from every locale dictionary.
// Parses with the TypeScript compiler (property-accurate ranges), deletes the
// full assignment including its trailing comma, and rewrites the file.
// Usage: bun run scripts/prune-i18n.ts <stale-keys-file>
// ============================================================
import { readFileSync, writeFileSync } from "node:fs";
import ts from "typescript";

const listFile = process.argv[2];
if (!listFile) {
  console.error("usage: prune-i18n.ts <stale-keys-file>");
  process.exit(2);
}

const stale = new Set(
  readFileSync(listFile, "utf8")
    .split("\n")
    .map((s) => s.trim())
    .filter((s) => s && !s.startsWith("error:")),
);

const FILES = ["en", "ar", "es", "hi", "pt", "ru", "vi", "zh"].map(
  (l) => `src/i18n/${l}.ts`,
);

let totalRemoved = 0;
const notFound: string[] = [];

for (const file of FILES) {
  const text = readFileSync(file, "utf8");
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const removals: Array<[number, number]> = [];
  const seen = new Set<string>();

  const visit = (node: ts.Node) => {
    if (ts.isPropertyAssignment(node)) {
      const name = node.name;
      const key = ts.isStringLiteral(name) || ts.isIdentifier(name) ? name.text : null;
      if (key && stale.has(key)) {
        seen.add(key);
        let end = node.getEnd();
        // swallow the trailing comma and the newline that follows it
        const rest = text.slice(end);
        const m = rest.match(/^\s*,?[ \t]*\r?\n/);
        if (m) end += m[0].length;
        removals.push([node.getFullStart(), end]);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);

  if (removals.length === 0) continue;

  removals.sort((a, b) => b[0] - a[0]);
  let out = text;
  for (const [from, to] of removals) out = out.slice(0, from) + out.slice(to);
  writeFileSync(file, out);

  const missing = [...stale].filter((k) => !seen.has(k));
  if (missing.length) notFound.push(`${file}: ${missing.length} not found (${missing.slice(0, 5).join(", ")}…)`);
  console.log(`${file}: removed ${removals.length}`);
  totalRemoved += removals.length;
}

console.log(`total removed: ${totalRemoved}`);
for (const nf of notFound) console.log(`note: ${nf}`);

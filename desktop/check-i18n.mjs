// Reconciles the keys in the code with the dictionary and hunts for Cyrillic
// left outside t().
//
// Needed because the key here is the Russian text itself: editing the copy
// silently breaks the link to the translation, and without a check that surfaces
// for the user rather than in CI.
//
//   node check-i18n.mjs          — report, non-zero exit when any are missing
//   node check-i18n.mjs --keys   — rewrite src/i18n/keys.json
import ts from "typescript";
import fs from "node:fs";
import path from "node:path";

const CYR = /[А-Яа-яЁё]/;
const root = "src";

// Keys that deliberately stay Russian: samples of Russian dictation, filler
// words and regex demonstrations. They have no English counterpart.
// Captions under the formatter rules: the example *is* the Russian filler the
// rule cleans out. The list has no English counterpart, and an invented English
// example would be worse than an honest Russian one.
const INTENTIONALLY_RUSSIAN = new Set([
  "ну, типа, как бы, в общем и свои слова ниже",
  "например: собственно\nскажем так",
  "я думаю что. я думаю что. я думаю что. -> я думаю что.",
  "я я хочу -> я хочу",
]);

// Files whose Cyrillic belongs entirely to the language of speech, not the UI.
const SPEECH_DOMAIN_FILES = new Set(["pages/aiShared.ts"]);

// A pinpoint exception written in the code itself. Needed where one and the
// same string cannot be judged by its text: the right-hand side of a ready-made
// replacement rule is a Russian word that is not translated.
const IGNORE_MARKER = "i18n-ignore";

function walkFiles(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walkFiles(p, out);
    else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name)) out.push(p);
  }
  return out;
}

const used = new Set();
const bare = [];

for (const file of walkFiles(root)) {
  const rel = path.relative(root, file).replace(/\\/g, "/");
  if (rel.startsWith("i18n/")) continue;
  const speechDomain = SPEECH_DOMAIN_FILES.has(rel);
  const text = fs.readFileSync(file, "utf8");
  const lines = text.split("\n").map((l) => l.replace(/\r$/, ""));
  const src = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  // The marker applies to its own line and the next one — so it works both at
  // the end of a line and as a standalone comment above a block.
  const ignored = (line) =>
    (lines[line - 1] ?? "").includes(IGNORE_MARKER) || (lines[line - 2] ?? "").includes(IGNORE_MARKER);

  const visit = (node) => {
    if (ts.isCallExpression(node)) {
      const fn = node.expression.getText(src);
      const [first, second] = node.arguments;
      if (fn === "t" && first && ts.isStringLiteralLike(first)) {
        used.add(first.text);
        ts.forEachChild(node, visit);
        return;
      }
      // tPlural(count, ["одна", "две", "пять"]) — the key is joined with |
      if (fn === "tPlural" && second && ts.isArrayLiteralExpression(second)) {
        const forms = second.elements.filter(ts.isStringLiteralLike).map((e) => e.text);
        if (forms.length === 3) used.add(forms.join("|"));
        ts.forEachChild(node, visit);
        return;
      }
    }
    // Cyrillic outside t(): either it was forgotten, or it is speech language.
    if (ts.isStringLiteralLike(node) && CYR.test(node.text)) {
      const inT =
        ts.isCallExpression(node.parent) &&
        ["t", "tPlural"].includes(node.parent.expression.getText(src));
      const inTPluralArray =
        ts.isArrayLiteralExpression(node.parent) &&
        ts.isCallExpression(node.parent.parent) &&
        node.parent.parent.expression.getText(src) === "tPlural";
      const line = src.getLineAndCharacterOfPosition(node.getStart(src)).line + 1;
      if (!inT && !inTPluralArray && !speechDomain && !ignored(line) && !INTENTIONALLY_RUSSIAN.has(node.text)) {
        bare.push({ file: rel, line, text: node.text.slice(0, 55) });
      }
    }
    if (ts.isJsxText(node) && CYR.test(node.text.trim())) {
      bare.push({ file: rel, line: src.getLineAndCharacterOfPosition(node.getStart(src)).line + 1, text: node.text.trim().slice(0, 55) });
    }
    ts.forEachChild(node, visit);
  };
  visit(src);
}

if (process.argv.includes("--keys")) {
  const sorted = [...used].sort((a, b) => a.localeCompare(b, "ru"));
  fs.writeFileSync("src/i18n/keys.json", JSON.stringify(sorted, null, 2) + "\n", "utf8");
  console.log(`keys.json переписан: ${sorted.length}`);
}

// The dictionary is read as text: a .ts file cannot be imported from node
// without a build, and all we need are the top-level keys.
const enSource = fs.readFileSync("src/i18n/en.ts", "utf8");
const translated = new Set();
for (const m of enSource.matchAll(/^\s{2}"((?:[^"\\]|\\.)*)":/gm)) {
  translated.add(JSON.parse(`"${m[1]}"`));
}

const missing = [...used].filter((k) => !translated.has(k) && !INTENTIONALLY_RUSSIAN.has(k)).sort((a, b) => a.localeCompare(b, "ru"));
const stale = [...translated].filter((k) => !used.has(k) && !INTENTIONALLY_RUSSIAN.has(k)).sort((a, b) => a.localeCompare(b, "ru"));

console.log(`ключей в коде: ${used.size}`);
console.log(`переведено:    ${translated.size}`);
console.log(`не переведено: ${missing.length}`);
console.log(`лишних в en:   ${stale.length}`);
console.log(`кириллица вне t(): ${bare.length}`);

if (missing.length) {
  console.log("\nбез перевода:");
  for (const k of missing) console.log(`  ${JSON.stringify(k)}`);
}
if (stale.length) {
  console.log("\nперевод есть, ключа в коде нет (копию правили после перевода?):");
  for (const k of stale) console.log(`  ${JSON.stringify(k)}`);
}
if (bare.length) {
  console.log("\nкириллица вне t():");
  for (const b of bare) console.log(`  ${b.file}:${b.line} ${JSON.stringify(b.text)}`);
}

process.exit(missing.length || stale.length || bare.length ? 1 : 0);

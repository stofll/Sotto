// Сверяет ключи в коде со словарём и ищет кириллицу, оставшуюся вне t().
//
// Нужен потому, что ключ здесь — сам русский текст: правка копии молча рвёт
// связь с переводом, и без проверки это всплывает у пользователя, а не в CI.
//
//   node check-i18n.mjs          — отчёт, ненулевой код при пропущенных
//   node check-i18n.mjs --keys   — переписать src/i18n/keys.json
import ts from "typescript";
import fs from "node:fs";
import path from "node:path";

const CYR = /[А-Яа-яЁё]/;
const root = "src";

// Ключи, которые намеренно остаются русскими: примеры русской диктовки,
// слова-паразиты и regex-демонстрации. Английского аналога у них нет.
// Подписи под правилами форматтера: пример — это и есть те русские паразиты,
// которые правило вычищает. Английского аналога у списка нет, и выдуманный
// английский пример был бы хуже честного русского.
const INTENTIONALLY_RUSSIAN = new Set([
  "ну, типа, как бы, в общем и свои слова ниже",
  "например: собственно\nскажем так",
  "я думаю что. я думаю что. я думаю что. -> я думаю что.",
  "я я хочу -> я хочу",
]);

// Файлы, чья кириллица целиком относится к языку речи, а не к интерфейсу.
const SPEECH_DOMAIN_FILES = new Set(["pages/aiShared.ts"]);

// Точечное исключение прямо в коде. Нужно там, где одну и ту же строку
// нельзя рассудить по тексту: правая часть готового правила замены —
// русское слово, которое не переводится.
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
  // Маркер действует на свою строку и на следующую — так он работает и в
  // конце строки, и отдельным комментарием над блоком.
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
      // tPlural(count, ["одна", "две", "пять"]) — ключ склеен через |
      if (fn === "tPlural" && second && ts.isArrayLiteralExpression(second)) {
        const forms = second.elements.filter(ts.isStringLiteralLike).map((e) => e.text);
        if (forms.length === 3) used.add(forms.join("|"));
        ts.forEachChild(node, visit);
        return;
      }
    }
    // Кириллица вне t(): либо забыли, либо это язык речи.
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

// Словарь читаем как текст: импортировать .ts из node без сборки нельзя,
// а нам нужны только ключи верхнего уровня.
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

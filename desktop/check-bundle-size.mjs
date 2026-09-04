// Бюджет стартовой загрузки для каждого окна.
//
// Считаем не размер отдельного чанка, а то, что окно реально скачивает до
// первого кадра: entry-скрипт из его HTML плюс все modulepreload, которые
// Vite туда положил, — это ровно статический граф импортов входа.
// Чанк сам по себе ничего не говорит: вынести половину кода в соседний
// файл, который всё равно грузится следом, — не улучшение.
//
// Порог — не факт из сборки, а решение. Если он мешает, его нужно
// подвинуть осознанно, вместе с ответом на вопрос «что выросло и зачем»,
// а не молча.
import { readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Килобайты здесь десятичные — как их печатает сам Vite, чтобы число из
// лога сборки и число отсюда можно было сравнивать глазами.
const BUDGETS_KB = {
  "index.html": 560,
  "overlay.html": 330,
  "tray.html": 330,
};

const distUrl = new URL("dist/", import.meta.url);

function assetsOf(html) {
  const refs = new Set();
  const patterns = [
    /<script[^>]+src="\/([^"]+\.js)"/g,
    /<link[^>]+rel="modulepreload"[^>]+href="\/([^"]+\.js)"/g,
  ];
  for (const re of patterns) {
    for (const m of html.matchAll(re)) refs.add(m[1]);
  }
  return [...refs];
}

let failed = false;
for (const [page, budgetKb] of Object.entries(BUDGETS_KB)) {
  const pageUrl = new URL(page, distUrl);
  let html;
  try {
    html = readFileSync(pageUrl, "utf8");
  } catch {
    console.error(`✗ ${page}: нет в dist/ — сборка не запускалась или вход переименован`);
    failed = true;
    continue;
  }
  const assets = assetsOf(html);
  if (assets.length === 0) {
    console.error(`✗ ${page}: не найдено ни одного JS — разметка входа изменилась, проверка ослепла`);
    failed = true;
    continue;
  }
  const bytes = assets.reduce(
    (sum, a) => sum + statSync(fileURLToPath(new URL(a, distUrl))).size,
    0
  );
  const kb = bytes / 1000;
  const mark = kb > budgetKb ? "✗" : "✓";
  console.log(
    `${mark} ${page}: ${kb.toFixed(1)} kB JS / бюджет ${budgetKb} kB (${assets.length} чанк(ов))`
  );
  if (kb > budgetKb) {
    for (const a of assets) {
      const size = statSync(fileURLToPath(new URL(a, distUrl))).size / 1000;
      console.log(`    ${size.toFixed(1)} kB  ${a}`);
    }
    failed = true;
  }
}

if (failed) {
  console.error(
    "\nСтартовая загрузка окна вышла за бюджет. Либо верните вес обратно " +
      "(dynamic import, вынос тяжёлой зависимости из общего графа), либо " +
      "поднимите порог в check-bundle-size.mjs осознанно и объясните почему."
  );
  process.exit(1);
}

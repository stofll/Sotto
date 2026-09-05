// The startup download budget for each window.
//
// What is measured is not the size of an individual chunk but what the window
// actually downloads before the first frame: the entry script from its HTML plus
// every modulepreload Vite put there — that is exactly the static import graph of
// the entry. A chunk on its own says nothing: moving half the code into a
// neighbouring file that loads right after it anyway is not an improvement.
//
// The threshold is a decision, not a fact from the build. If it gets in the way
// it should be moved deliberately, together with an answer to "what grew and
// why", rather than silently.
import { readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";

// The kilobytes here are decimal — the way Vite prints them itself, so that the
// number from the build log and the number from here can be compared by eye.
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

// Ищет вызовы t() в инициализаторах констант уровня модуля.
//
// Такой вызов выполняется один раз при импорте — до того, как приложение
// узнало язык из конфига, — и навсегда застревает на языке по умолчанию.
// Внутри компонента t() вычисляется при каждой отрисовке и потому переключается.
import ts from "typescript";
import fs from "node:fs";
import path from "node:path";

function walkFiles(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walkFiles(p, out);
    else if (/\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name)) out.push(p);
  }
  return out;
}

const hits = [];
for (const file of walkFiles("src")) {
  const rel = path.relative("src", file).replace(/\\/g, "/");
  if (rel.startsWith("i18n/")) continue;
  const src = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

  for (const stmt of src.statements) {
    if (!ts.isVariableStatement(stmt)) continue;
    for (const decl of stmt.declarationList.declarations) {
      if (!decl.initializer) continue;
      // t() внутри функции/стрелки вычислится при вызове — это нормально.
      let eager = false;
      const scan = (node) => {
        if (ts.isArrowFunction(node) || ts.isFunctionExpression(node)) return;
        if (ts.isCallExpression(node) && ["t", "tPlural"].includes(node.expression.getText(src))) eager = true;
        ts.forEachChild(node, scan);
      };
      scan(decl.initializer);
      if (eager) {
        hits.push({
          file: rel,
          line: src.getLineAndCharacterOfPosition(decl.getStart(src)).line + 1,
          name: decl.name.getText(src),
        });
      }
    }
  }
}

for (const h of hits) console.log(`${h.file}:${h.line} ${h.name}`);
console.log(`\nвсего: ${hits.length}`);
process.exit(hits.length ? 1 : 0);

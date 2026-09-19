import ts from "typescript";
import { expect, it } from "vitest";

const sources = import.meta.glob<string>("../**/*.tsx", { query: "?raw", import: "default", eager: true });

it("uses styled hints instead of native title tooltips throughout the interface", () => {
  const violations: string[] = [];
  for (const [path, text] of Object.entries(sources)) {
    const source = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    function visit(node: ts.Node) {
      if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && /^[a-z]/.test(node.tagName.getText(source))) {
        if (node.attributes.properties.some(attribute => ts.isJsxAttribute(attribute) && attribute.name.getText(source) === "title")) {
          violations.push(`${path}:${source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1}`);
        }
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
  }
  expect(Object.keys(sources).length).toBeGreaterThan(0);
  expect(violations).toEqual([]);
});

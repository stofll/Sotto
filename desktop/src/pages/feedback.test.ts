import { expect, it } from "vitest";
import { issueUrl } from "./feedback";

it("encodes the reviewed summary without changing the destination or query", () => {
  const summary = "Sotto: 1.0\nModel: tiny & # ? тест";
  const url = new URL(issueUrl("bug", summary));
  expect(url.origin).toBe("https://github.com");
  expect(url.pathname).toBe("/stofll/Sotto/issues/new");
  expect(url.searchParams.get("template")).toBe("bug_report.md");
  expect(url.searchParams.get("body")).toContain(summary);
  expect(url.hash).toBe("");
});
it("leaves the template intact when diagnostics are omitted", () => {
  expect(new URL(issueUrl("bug")).searchParams.has("body")).toBe(false);
  const url = new URL(issueUrl("feature", "must not be sent"));
  expect(url.searchParams.get("template")).toBe("feature_request.md");
  expect(url.searchParams.has("body")).toBe(false);
});

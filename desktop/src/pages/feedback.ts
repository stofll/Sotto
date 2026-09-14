export const ISSUES_URL = "https://github.com/stofll/Sotto/issues";

export function issueUrl(kind: "bug" | "feature", summary = ""): string {
  const query = new URLSearchParams({ template: kind === "bug" ? "bug_report.md" : "feature_request.md" });
  if (kind === "bug" && summary) {
    query.set("body", `## Summary\n\n\n## Steps to reproduce\n\n1. \n\n## Expected behavior\n\n\n## Actual behavior\n\n\n## Environment\n\n${summary}\nOS version: \nInstallation source: \n\n## Logs or screenshots\n\n<!-- Attach the reviewed public diagnostic export here. Do not attach recordings or raw logs. -->`);
  }
  return `${ISSUES_URL}/new?${query}`;
}

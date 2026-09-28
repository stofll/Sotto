import { t } from "../i18n";

// Logs and recordings range from kilobytes to gigabytes. "0.0 MB" on a fresh
// install reports nothing, so small values are shown in kilobytes.
export function formatFileSize(bytes: number) {
  return bytes < 1024 * 1024
    ? t("{p0} КБ", { p0: Math.round(bytes / 1024).toString() })
    : t("{p0} МБ", { p0: (bytes / 1024 / 1024).toFixed(1) });
}

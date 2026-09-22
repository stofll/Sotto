import { invoke } from "./invoke";
import { on } from "./events";

export type SetupPhase = "ready" | "preparing" | "installing" | "complete" | "failed";
export interface SetupStatus {
  phase: SetupPhase;
  revision: number;
  version: string;
  preview: boolean;
  error: string | null;
}
export interface SetupInstallOptions {
  install_dir: string;
  desktop_shortcut: boolean;
  start_menu_shortcut: boolean;
}
export interface SetupDefaults {
  options: SetupInstallOptions;
  directory_locked: boolean;
}
export const setupStatus = () => invoke<SetupStatus>("setup_status");
export const setupOptions = () => invoke<SetupDefaults>("setup_options");
export const chooseSetupDirectory = (directory: string) => invoke<string | null>("setup_choose_directory", { directory });
export const installSotto = (options: SetupInstallOptions) => invoke<void>("setup_install", { options });
export const launchSotto = () => invoke<void>("setup_launch");
export const closeSetup = () => invoke<void>("setup_close");
export const onSetupStatus = (handler: (state: SetupStatus) => void) => on("setup-status", handler);

/** A snapshot requested before an event may arrive after it. Never rewind the UI. */
export function newestSetupStatus(current: SetupStatus | null, next: SetupStatus): SetupStatus {
  return current && current.revision > next.revision ? current : next;
}

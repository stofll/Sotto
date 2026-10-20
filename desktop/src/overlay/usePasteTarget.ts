import { useEffect, useState } from "react";
import { invoke } from "../bridge/invoke";
import { targetLabel } from "./recordingFigures";

/**
 * The application the text will go to, asked once as each recording starts
 * and only while a recipe shows it, so a recipe without the element never
 * makes the native side look at the focused application. Nothing is kept:
 * the name lives in this state and goes with the overlay.
 */
export function usePasteTarget(wanted: boolean, recordingKey: number | null) {
  const [name, setName] = useState("");
  useEffect(() => {
    setName("");
    if (!wanted || recordingKey === null) return;
    let stale = false;
    invoke<string | null>("paste_target_name").then((raw) => { if (!stale) setName(targetLabel(raw)); }).catch(() => {});
    return () => { stale = true; };
  }, [wanted, recordingKey]);
  return name;
}

import { useCallback, useEffect, useRef, useState } from "react";
import { getModelAssessments, type ModelAssessment } from "../bridge/modelAssessments";

/** `enabled: false` keeps a mounted but hidden caller from fetching. */
export function useModelAssessments(context: unknown, enabled = true) {
  const [values, setValues] = useState<Record<string, ModelAssessment>>({});
  const generation = useRef(0);
  const refresh = useCallback(() => {
    const request = ++generation.current;
    getModelAssessments().then((rows) => {
      if (request === generation.current) setValues(Object.fromEntries(rows.map((row) => [row.id, row])));
    }).catch(() => { if (request === generation.current) setValues({}); });
  }, []);
  useEffect(() => {
    setValues({});
    if (!enabled) return;
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request generation counter, not a DOM node: cleanup must bump the current value.
    return () => { generation.current++; };
  }, [context, enabled, refresh]);
  useEffect(() => {
    if (!enabled) return;
    window.addEventListener("focus", refresh);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request generation counter, as above.
    return () => { window.removeEventListener("focus", refresh); generation.current++; };
  }, [enabled, refresh]);
  return { values };
}

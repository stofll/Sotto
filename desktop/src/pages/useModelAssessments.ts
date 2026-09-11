import { useCallback, useEffect, useRef, useState } from "react";
import { getModelAssessments, type ModelAssessment } from "../bridge/modelAssessments";
import { on } from "../bridge/events";

export function useModelAssessments(context: unknown) {
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
    refresh();
    return () => { generation.current++; };
  }, [context, refresh]);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    on("model-performance-changed", refresh).then((stop) => {
      if (cancelled) stop(); else unlisten = stop;
    }).catch(() => {});
    window.addEventListener("focus", refresh);
    return () => { cancelled = true; unlisten?.(); window.removeEventListener("focus", refresh); generation.current++; };
  }, [refresh]);
  return { values, refresh };
}

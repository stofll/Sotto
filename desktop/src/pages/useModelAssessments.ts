import { useCallback, useEffect, useRef, useState } from "react";
import { getModelAssessments, type ModelAssessment } from "../bridge/modelAssessments";
import { subscribe } from "../bridge/events";

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
    const unlisten = subscribe("model-performance-changed", refresh);
    window.addEventListener("focus", refresh);
    return () => { unlisten(); window.removeEventListener("focus", refresh); generation.current++; };
  }, [refresh]);
  return { values, refresh };
}

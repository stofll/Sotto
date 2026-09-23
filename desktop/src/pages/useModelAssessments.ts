import { useCallback, useEffect, useRef, useState } from "react";
import { getModelAssessments, type ModelAssessment } from "../bridge/modelAssessments";

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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request generation counter, not a DOM node: cleanup must bump the current value.
    return () => { generation.current++; };
  }, [context, refresh]);
  useEffect(() => {
    window.addEventListener("focus", refresh);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request generation counter, as above.
    return () => { window.removeEventListener("focus", refresh); generation.current++; };
  }, [refresh]);
  return { values };
}

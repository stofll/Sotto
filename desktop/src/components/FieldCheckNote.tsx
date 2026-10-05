/** The line under a field that says what is wrong with its value, shared by the
 *  wizard and the profile editor so a check reads the same in both. An empty
 *  field is a hint, not an error: nothing has been got wrong yet. */
export function FieldCheckNote({ id, check, value }: {
  id: string;
  check: { level: "error" | "warn"; message: string } | null;
  value: string;
}) {
  if (!check) return null;
  const empty = !value.trim();
  return (
    <div id={id} className="field-check" data-tone={empty ? "hint" : check.level}
      role={check.level === "error" && !empty ? "alert" : "status"}>
      {check.message}
    </div>
  );
}

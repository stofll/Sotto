/**
 * Filtering dropdown items by what was typed.
 *
 * A separate module rather than living inside the component: a search rule is
 * the kind of thing that breaks silently (the wrong item matched, the right one
 * missed), and it has to be testable without React.
 */
export type SearchableOption = { label: string; meta?: string };

/**
 * Whether an item matches what was typed.
 *
 * We search both the label and the code: people know languages differently —
 * one types «нем», another «de». Case is folded with the locale-aware call,
 * otherwise the Turkish «i» does not match the way it looks.
 */
export function optionMatches(option: SearchableOption, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;
  const haystack = `${option.label} ${option.meta ?? ""}`.toLocaleLowerCase();
  return haystack.includes(needle);
}

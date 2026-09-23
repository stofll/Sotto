/**
 * Items for a minutes dropdown whose `0` means "off".
 *
 * The config is also edited by hand, and a number from there may match no item
 * at all. In that case the item is added rather than replaced by the nearest
 * one: the setting works exactly as written, and the list has to show that —
 * otherwise merely opening settings would silently rewrite it.
 */
export function minuteChoices(choices: readonly number[], current: number): number[] {
  const minutes = [...new Set([...choices, current])].filter((value) => value > 0);
  minutes.sort((a, b) => a - b);
  // "Off" goes last: it is not the longest interval but the refusal of one.
  return [...minutes, 0];
}

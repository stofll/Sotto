/**
 * Отбор пунктов выпадающего списка по набранному.
 *
 * Отдельным модулем, а не внутри компонента: правило поиска — это то, что
 * ломается незаметно (нашлось не то, не нашлось нужное), и проверять его
 * надо без React.
 */
export type SearchableOption = { label: string; meta?: string };

/**
 * Подходит ли пункт под набранное.
 *
 * Ищем и по названию, и по коду: язык знают по-разному — кто-то наберёт
 * «нем», кто-то «de». Регистр приводим локально, иначе турецкая «i» ищется
 * не так, как выглядит.
 */
export function optionMatches(option: SearchableOption, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;
  const haystack = `${option.label} ${option.meta ?? ""}`.toLocaleLowerCase();
  return haystack.includes(needle);
}

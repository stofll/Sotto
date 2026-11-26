import type { DictionarySet } from "../bridge/types";
import { t } from "../i18n";

export const MAX_DICTIONARY_FILE_BYTES = 2 * 1024 * 1024;

export type DictionaryImport = {
  name: string;
  description: string;
  words: string[];
  total: number;
  duplicates: number;
};

type ImportProblem = "size" | "encoding" | "format" | "json" | "version" | "schema" | "name" | "empty" | "term" | "linebreak" | "length" | "word_count";

export class DictionaryImportError extends Error {
  constructor(readonly problem: ImportProblem, readonly position?: number) {
    super(problem);
  }
}

export function dictionaryImportErrorMessage(error: unknown): string {
  if (!(error instanceof DictionaryImportError)) return t("Не удалось прочитать файл. Проверьте доступ к нему и попробуйте ещё раз.");
  const position = { position: error.position ?? 0 };
  switch (error.problem) {
    case "size": return t("Файл словаря должен быть не больше 2 МиБ.");
    case "encoding": return t("Не удалось прочитать текст в UTF-8. Сохраните файл в кодировке UTF-8 и повторите импорт.");
    case "format": return t("Выберите файл .txt или .json.");
    case "json": return t("Некорректный JSON. Проверьте синтаксис файла и повторите импорт.");
    case "version": return t("Поддерживается только версия 1 файла словаря.");
    case "schema": return t("JSON словаря должен содержать version: 1, name, description и массив строк words.");
    case "name": return t("В JSON словаря должно быть непустое название name.");
    case "empty": return t("В текстовом файле нет терминов. Добавьте по одному термину в строке.");
    case "term": return t("Термин №{position} должен быть непустой строкой.", position);
    case "linebreak": return t("Термин №{position} содержит перенос строки. Каждый элемент words должен содержать один термин.", position);
    case "length": return t("Термин №{position} длиннее 128 символов. Сократите его и повторите импорт.", position);
    case "word_count": return t("Термин №{position} содержит больше 8 слов. Сократите его и повторите импорт.", position);
  }
}

function validateTerm(value: unknown, position: number): string {
  if (typeof value !== "string" || !value.trim()) throw new DictionaryImportError("term", position);
  if (/[\r\n]/.test(value)) throw new DictionaryImportError("linebreak", position);
  const term = value.trim();
  if ([...term].length > 128) throw new DictionaryImportError("length", position);
  if (term.split(/\s+/u).length > 8) throw new DictionaryImportError("word_count", position);
  return term;
}

/** Terms stay in file order; case variants need the library's spelling choice. */
export function parseDictionaryImport(text: string, filename: string): DictionaryImport {
  if (new TextEncoder().encode(text).length > MAX_DICTIONARY_FILE_BYTES) throw new DictionaryImportError("size");
  const contents = text.replace(/^\uFEFF/u, "");
  let name = filename.replace(/\.(?:txt|json)$/i, "").trim();
  let description = "";
  let terms: string[];
  if (/\.json$/i.test(filename)) {
    let value: unknown;
    try { value = JSON.parse(contents); } catch { throw new DictionaryImportError("json"); }
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new DictionaryImportError("schema");
    const document = value as Record<string, unknown>;
    if (document.version !== 1) throw new DictionaryImportError("version");
    if (Object.keys(document).some((key) => !["version", "name", "description", "words"].includes(key))
      || typeof document.name !== "string" || typeof document.description !== "string" || !Array.isArray(document.words)) throw new DictionaryImportError("schema");
    if (!document.name.trim()) throw new DictionaryImportError("name");
    name = document.name.trim();
    description = document.description;
    terms = document.words.map((value, index) => validateTerm(value, index + 1));
  } else if (/\.txt$/i.test(filename)) {
    terms = contents.split(/\r\n|\n|\r/u).flatMap((line, index) => line.trim() ? [validateTerm(line, index + 1)] : []);
    if (!terms.length) throw new DictionaryImportError("empty");
  } else throw new DictionaryImportError("format");
  const words = [...new Set(terms)];
  return { name, description, words, total: terms.length, duplicates: terms.length - words.length };
}

export async function readDictionaryFile(file: Pick<File, "name" | "size" | "arrayBuffer">): Promise<DictionaryImport> {
  if (file.size > MAX_DICTIONARY_FILE_BYTES) throw new DictionaryImportError("size");
  const bytes = await file.arrayBuffer();
  let text: string;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { throw new DictionaryImportError("encoding"); }
  return parseDictionaryImport(text, file.name);
}

export function mergeDictionaryImport(current: string[], imported: DictionaryImport): { words: string[]; added: number; duplicates: number } {
  const words = [...new Set([...current, ...imported.words])];
  const added = words.length - new Set(current).size;
  return { words, added, duplicates: imported.total - added };
}

/** IDs and activation belong to this installation, never to an exchanged set. */
export function serializeDictionarySet(set: Pick<DictionarySet, "name" | "description" | "words">): string {
  return JSON.stringify({ version: 1, name: set.name, description: set.description, words: set.words }, null, 2);
}

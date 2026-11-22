import { describe, expect, it, vi } from "vitest";
import { MAX_DICTIONARY_FILE_BYTES, mergeDictionaryImport, parseDictionaryImport, readDictionaryFile, serializeDictionarySet } from "./dictionaryImport";
import { parseDictionaryWords } from "./dictionarySets";

function json(words: unknown, patch: Record<string, unknown> = {}): string {
  return JSON.stringify({ version: 1, name: "Medical terms", description: "Synthetic vocabulary", words, ...patch });
}

describe("dictionary file import", () => {
  it("reads UTF-8 BOM and Windows lines, preserves phrases and commas, and reports exact duplicates", () => {
    expect(parseDictionaryImport("\uFEFF Сложный термин, вариант 2\r\n FLAIR\r\n\r\nFLAIR\r\nflair\r\n", "Medical.TXT")).toEqual({
      name: "Medical", description: "", words: ["Сложный термин, вариант 2", "FLAIR", "flair"], total: 4, duplicates: 1,
    });
  });

  it("imports 2000 terms without truncation and keeps all terms through the editor", () => {
    const words = Array.from({ length: 2000 }, (_, index) => `Синтетический термин ${index}`);
    const imported = parseDictionaryImport(json(words), "terms.json");
    expect(imported.words).toEqual(words);
    expect(parseDictionaryWords(imported.words.join("\n"))).toEqual(words);
    expect(imported.total).toBe(2000);
  });

  it("round-trips one versioned set without transferring identity or activation", () => {
    const set = { id: "local-id", name: "Example", description: "A description", enabled: true, words: ["тип 1, вариант 2", "МРТ", "мрт"] };
    const content = serializeDictionarySet(set);
    expect(JSON.parse(content)).toEqual({ version: 1, name: set.name, description: set.description, words: set.words });
    expect(parseDictionaryImport(content, "terms.json")).toEqual({ name: set.name, description: set.description, words: set.words, total: 3, duplicates: 0 });
    expect(parseDictionaryImport(json([]), "empty.json").words).toEqual([]);
  });

  it.each([
    ["not JSON", "json"],
    ["[]", "schema"],
    ["null", "schema"],
    [json([], { version: 2 }), "version"],
    [json([], { version: "1" }), "version"],
    [json([], { name: "  " }), "name"],
    [json([], { description: null }), "schema"],
    [json([], { enabled: true }), "schema"],
    [json("word"), "schema"],
  ])("rejects malformed or unsupported documents without coercion (%s)", (content, problem) => {
    expect(() => parseDictionaryImport(content, "terms.json")).toThrow(expect.objectContaining({ problem }));
  });

  it.each([
    [null, "term"],
    ["  ", "term"],
    ["one\ntwo", "linebreak"],
    ["one\rtwo", "linebreak"],
    ["я".repeat(129), "length"],
    [Array(9).fill("word").join(" "), "word_count"],
  ])("identifies the offending JSON term (%s)", (term, problem) => {
    expect(() => parseDictionaryImport(json(["valid", term]), "terms.json")).toThrow(expect.objectContaining({ problem, position: 2 }));
  });

  it("counts Unicode characters like Rust and reports physical TXT line numbers", () => {
    expect(parseDictionaryImport("😀".repeat(128), "terms.txt").words).toEqual(["😀".repeat(128)]);
    expect(parseDictionaryImport(Array(8).fill("word").join(" "), "terms.txt").words).toHaveLength(1);
    expect(() => parseDictionaryImport(`valid\r\n\r\n${"я".repeat(129)}`, "terms.txt")).toThrow(expect.objectContaining({ problem: "length", position: 3 }));
    expect(() => parseDictionaryImport(" \r\n\n", "terms.txt")).toThrow(expect.objectContaining({ problem: "empty" }));
    expect(() => parseDictionaryImport("word", "terms.csv")).toThrow(expect.objectContaining({ problem: "format" }));
  });

  it("merges into the current draft without replacing its terms or collapsing case conflicts", () => {
    const current = ["Existing, phrase", "FLAIR"];
    const imported = parseDictionaryImport("FLAIR\nnew term\nnew term\nflair", "terms.txt");
    expect(mergeDictionaryImport(current, imported)).toEqual({ words: ["Existing, phrase", "FLAIR", "new term", "flair"], added: 2, duplicates: 2 });
    expect(current).toEqual(["Existing, phrase", "FLAIR"]);
    expect(imported.words).toEqual(["FLAIR", "new term", "flair"]);
  });

  it("rejects an oversized file before reading and invalid UTF-8 instead of silently changing terms", async () => {
    const arrayBuffer = vi.fn();
    await expect(readDictionaryFile({ name: "terms.txt", size: MAX_DICTIONARY_FILE_BYTES + 1, arrayBuffer })).rejects.toMatchObject({ problem: "size" });
    expect(arrayBuffer).not.toHaveBeenCalled();
    await expect(readDictionaryFile({ name: "terms.txt", size: 2, arrayBuffer: async () => new Uint8Array([0xff, 0xfe]).buffer })).rejects.toMatchObject({ problem: "encoding" });
    const bytes = new TextEncoder().encode("\uFEFFтермин\r\n");
    await expect(readDictionaryFile({ name: "terms.txt", size: bytes.length, arrayBuffer: async () => bytes.buffer })).resolves.toMatchObject({ words: ["термин"] });
  });
});

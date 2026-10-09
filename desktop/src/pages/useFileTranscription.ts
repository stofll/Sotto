import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, on } from "../bridge";
import type { HistoryEntry } from "../bridge/types";

/** The processing stage of an attached file. The engine reports no progress, so
 *  the UI shows decoding or recognition. */
export type FileStage = null | "decoding" | "transcribing";

/** The response of `transcribe_audio_file`. A different shape from
 *  `AiRunResult`: that is the result of one LLM call, this is the entire
 *  recognition pipeline. */
export type TranscribeFileResult = {
  text: string;
  raw_text: string;
  formatted_text: string;
  /** The same status history stores for a dictation. */
  ai_status: HistoryEntry["ai_processing"] | null;
  audio_seconds: number;
  inference_time_ms: number;
  language: string | null;
};


type Operation = { sessionId: number | null; cancelled: boolean };
const message = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Own the native session and subscription through decoding and recognition. */
export function useFileTranscription() {
  const [fileStage, setFileStage] = useState<FileStage>(null);
  const [fileResult, setFileResult] = useState<TranscribeFileResult | null>(null);
  const [fileError, setFileError] = useState("");
  const mounted = useRef(false);
  const active = useRef<Operation | null>(null);

  const cancel = useCallback(async (operation: Operation) => {
    operation.cancelled = true;
    if (operation.sessionId === null) return;
    try {
      await invoke("cancel_audio_file", { session_id: operation.sessionId });
    } catch (error) {
      if (mounted.current && active.current === operation) setFileError(message(error));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (active.current) void cancel(active.current);
    };
  }, [cancel]);

  const transcribeFile = useCallback(async (path: string) => {
    if (!mounted.current || active.current) return;
    const operation: Operation = { sessionId: null, cancelled: false };
    active.current = operation;
    setFileError("");
    setFileResult(null);
    setFileStage("decoding");
    let unlisten: (() => void) | undefined;
    try {
      // Keep listening until the command settles: unmount may precede its first event.
      unlisten = await on<{ session_id: number; stage?: Exclude<FileStage, null> }>(
        "file-transcription-started", (payload) => {
          if (operation.sessionId !== null && operation.sessionId !== payload.session_id) return;
          operation.sessionId = payload.session_id;
          if (operation.cancelled) void cancel(operation);
          else if (mounted.current) setFileStage(payload.stage ?? "transcribing");
        },
      );
      if (operation.cancelled || !mounted.current) return;
      const result = await invoke<TranscribeFileResult>("transcribe_audio_file", { path });
      if (mounted.current && !operation.cancelled) setFileResult(result);
    } catch (error) {
      if (mounted.current) setFileError(message(error));
    } finally {
      unlisten?.();
      if (active.current === operation) active.current = null;
      if (mounted.current) setFileStage(null);
    }
  }, [cancel]);

  async function runFileTranscription() {
    if (active.current) return;
    setFileError("");
    setFileResult(null);
    try {
      const path = await invoke<string | null>("pick_audio_file");
      if (path && mounted.current) await transcribeFile(path);
    } catch (error) {
      if (mounted.current) setFileError(message(error));
    }
  }

  async function cancelFileTranscription() {
    if (active.current) await cancel(active.current);
  }

  return { fileStage, fileResult, fileError, setFileResult, setFileError,
    transcribeFile, cancelFileTranscription, runFileTranscription };
}

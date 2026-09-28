import { useCallback, useEffect, useRef, useState } from "react";

export type PlayerState = {
  id: number;
  status: "loading" | "playing" | "paused";
  position: number;
  duration: number;
} | null;

/**
 * One saved recording at a time for the History page.
 *
 * The audio arrives as WAV bytes and plays from a blob URL, so the page needs
 * no file access. Starting another entry stops the current one; a load that
 * is overtaken by another click is dropped when it arrives.
 */
export function useRecordingPlayer(load: (id: number) => Promise<ArrayBuffer>, onError: (message: string) => void) {
  const [state, setState] = useState<PlayerState>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const urlRef = useRef<string | null>(null);
  const requestRef = useRef(0);

  const stop = useCallback(() => {
    requestRef.current += 1;
    const audio = audioRef.current;
    audioRef.current = null;
    if (audio) {
      audio.onended = audio.ontimeupdate = audio.onloadedmetadata = audio.onpause = audio.onplay = null;
      audio.pause();
    }
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    urlRef.current = null;
    setState(null);
  }, []);

  useEffect(() => stop, [stop]);

  const start = useCallback(async (id: number) => {
    stop();
    const request = requestRef.current;
    setState({ id, status: "loading", position: 0, duration: 0 });
    try {
      const bytes = await load(id);
      if (requestRef.current !== request) return;
      const url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
      urlRef.current = url;
      const audio = new Audio(url);
      audioRef.current = audio;
      const update = (status?: "playing" | "paused") => setState((current) => current && current.id === id ? {
        id,
        status: status ?? current.status,
        position: audio.currentTime,
        duration: Number.isFinite(audio.duration) ? audio.duration : current.duration,
      } : current);
      audio.onloadedmetadata = () => update();
      audio.ontimeupdate = () => update();
      audio.onplay = () => update("playing");
      audio.onpause = () => update("paused");
      audio.onended = () => stop();
      await audio.play();
    } catch (e) {
      if (requestRef.current !== request) return;
      stop();
      onError(e instanceof Error ? e.message : String(e));
    }
  }, [load, onError, stop]);

  const toggle = useCallback((id: number) => {
    const audio = audioRef.current;
    if (state?.id !== id) { void start(id); return; }
    if (!audio) { stop(); return; }
    if (audio.paused) void audio.play().catch((e) => onError(e instanceof Error ? e.message : String(e)));
    else audio.pause();
  }, [state?.id, start, stop, onError]);

  const seek = useCallback((seconds: number) => {
    const audio = audioRef.current;
    if (!audio || !Number.isFinite(audio.duration)) return;
    audio.currentTime = Math.max(0, Math.min(audio.duration, seconds));
  }, []);

  return { state, toggle, seek, stop };
}

export function formatPlayerTime(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

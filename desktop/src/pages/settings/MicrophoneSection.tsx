import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, subscribe as subscribeEvent } from "../../bridge";
import { CustomSelect } from "../../components/CustomSelect";
import { Icon } from "../../components/Icon";
import { Hint } from "../../components/Hint";
import { t } from "../../i18n";
import type { MicrophoneResult } from "../../bridge/types";
import type { ConfigChanged } from "./controls";

/** A device whose name cpal could not read still has to be called something. */
function microphoneLabel(mic: MicrophoneResult): string {
  return mic.name || mic.label || t("Микрофон {p0}", { p0: (mic.index ?? 0) + 1 });
}

const MIC_SEGMENTS = 24;

function MicMeter({ level, peak, active }: { level: number; peak: number; active: boolean }) {
  // Left-to-right VU meter: the lit length tracks the current loudness, so the
  // test tells you how well (and how loudly) you're heard — not just that the
  // mic is alive. Colour zones flag quiet / good / too-hot, and a peak-hold
  // marker keeps the recent maximum visible. This intentionally differs from
  // the recording overlay's rolling waveform.
  const clamped = Math.min(1, Math.max(0, level));
  const litCount = Math.round(clamped * MIC_SEGMENTS);
  const peakIndex = peak > 0 ? Math.min(MIC_SEGMENTS - 1, Math.max(0, Math.ceil(peak * MIC_SEGMENTS) - 1)) : -1;
  return (
    <div
      className={`mic-meter${active ? " mic-meter--active" : ""}`}
      role="meter"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(clamped * 100)}
    >
      {Array.from({ length: MIC_SEGMENTS }, (_, index) => {
        const fraction = (index + 1) / MIC_SEGMENTS;
        const zone = fraction > 0.88 ? "hot" : fraction > 0.7 ? "warn" : "ok";
        const lit = index < litCount;
        const isPeak = index === peakIndex && !lit;
        return (
          <span
            key={index}
            data-zone={zone}
            className={`mic-meter__seg${lit ? " is-lit" : ""}${isPeak ? " is-peak" : ""}`}
          />
        );
      })}
    </div>
  );
}

const MIC_ACTIVE_RAMP_UP = 0.08;
const MIC_ACTIVE_RAMP_DOWN = 0.03;
// VU dynamics at the backend's ~40 ms tick: fast attack so peaks register,
// slower release so the bar glides back down instead of flickering.
const MIC_ATTACK_ALPHA = 0.55;
const MIC_RELEASE_ALPHA = 0.18;
// Peak-hold decay per tick — the marker falls from full to zero in ~2.5 s.
const MIC_PEAK_DECAY = 0.016;

function nextActive(prev: boolean, level: number): boolean {
  if (!prev && level >= MIC_ACTIVE_RAMP_UP) return true;
  if (prev && level <= MIC_ACTIVE_RAMP_DOWN) return false;
  return prev;
}

type AppErrorPayload = { kind?: string; permission?: string; hint?: string; message?: string };

export function MicPicker({ microphone, microphones, onConfigChanged }: { microphone?: string | number | null; microphones: MicrophoneResult[]; onConfigChanged: ConfigChanged }) {
  // The level check and echo are two different modes on one capture stream.
  // There used to be a single button turning both on at once: to look at the
  // meter you had to listen to yourself through the speakers and catch the
  // feedback. Hence two flags, while the stream lives as long as at least one of
  // them is on.
  const [checking, setChecking] = useState(false);
  const [echo, setEcho] = useState(false);
  const [busy, setBusy] = useState(false);
  const [devices, setDevices] = useState(microphones);
  const playback = useRef<AudioContext | null>(null);
  const nextPlayback = useRef(0);

  function closePlayback() {
    const context = playback.current;
    playback.current = null;
    nextPlayback.current = 0;
    if (context) void context.close();
  }

  // The device list is re-read on an event rather than on a timer: polling every
  // three seconds drove cpal enumeration on the audio thread the whole time the
  // settings were open — including with the window minimised to the tray. Both
  // moments when a microphone plugged in mid-session should appear are caught
  // precisely: focus returning to the window and the list itself being opened.
  const pendingRefresh = useRef(false);
  const disposed = useRef(false);
  const refreshDevices = useCallback(async () => {
    if (pendingRefresh.current) return;
    pendingRefresh.current = true;
    try {
      const list = await invoke<MicrophoneResult[]>("list_microphones");
      if (!disposed.current) setDevices(list);
    } catch { /* Keep the last successful enumeration during a reconnect. */ }
    finally { pendingRefresh.current = false; }
  }, []);

  useEffect(() => {
    disposed.current = false;
    const onFocus = () => void refreshDevices();
    void refreshDevices();
    window.addEventListener("focus", onFocus);
    return () => {
      disposed.current = true;
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshDevices]);
  const [level, setLevel] = useState(0);
  const [peak, setPeak] = useState(0);
  const [micActive, setMicActive] = useState(false);
  const smoothRef = useRef(0);
  const peakRef = useRef(0);
  const prevActiveRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<{ text: string; kind: "running" | "stopped" } | null>(null);
  const statusTimerRef = useRef<number | null>(null);

  function resetMeter() {
    smoothRef.current = 0;
    peakRef.current = 0;
    prevActiveRef.current = false;
    setLevel(0);
    setPeak(0);
    setMicActive(false);
  }
  const options = [{ label: t("Системный микрофон по умолчанию"), value: null as string | number | null }, ...devices.map((mic) => ({ label: microphoneLabel(mic), value: mic.id ?? mic.index ?? null }))];

  const selectedValue = typeof microphone === "number" || (typeof microphone === "string" && /^\d+$/.test(microphone))
    ? devices.find((mic) => mic.index === Number(microphone))?.id ?? microphone
    : microphone ?? null;
  if (selectedValue !== null && !options.some((option) => option.value === selectedValue)) {
    options.push({ label: t("Микрофон отключён"), value: selectedValue });
  }

  function setRunningStatus(text: string) {
    if (statusTimerRef.current !== null) {
      window.clearTimeout(statusTimerRef.current);
      statusTimerRef.current = null;
    }
    setStatus({ text, kind: "running" });
  }

  function setStoppedStatus(text: string, ttlMs = 1500) {
    if (statusTimerRef.current !== null) {
      window.clearTimeout(statusTimerRef.current);
      statusTimerRef.current = null;
    }
    setStatus({ text, kind: "stopped" });
    statusTimerRef.current = window.setTimeout(() => {
      statusTimerRef.current = null;
      setStatus((current) => (current?.kind === "stopped" ? null : current));
    }, ttlMs);
  }

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const subscribe = <T,>(event: string, handler: (payload: T) => void) => {
      unlisteners.push(subscribeEvent(event, handler));
    };
    // The rate comes with the samples rather than being assumed: capture
    // resamples to 16 kHz only when the device rate divides into it, and on a
    // 44.1 kHz microphone the frames arrive at the device's own rate.
    subscribe<{ sample_rate?: number; samples?: number[] }>("microphone-test-audio", (payload) => {
      const context = playback.current;
      const samples = payload?.samples;
      const sampleRate = payload?.sample_rate;
      if (!context || context.state !== "running" || !samples?.length || !sampleRate) return;
      // Bound queued playback when the webview was stalled or hidden.
      if (nextPlayback.current > context.currentTime + 0.25) return;
      const buffer = context.createBuffer(1, samples.length, sampleRate);
      buffer.copyToChannel(new Float32Array(samples), 0);
      const source = context.createBufferSource();
      source.buffer = buffer;
      source.connect(context.destination);
      const at = Math.max(context.currentTime + 0.02, nextPlayback.current);
      source.start(at);
      nextPlayback.current = at + buffer.duration;
    });
    subscribe<{ level: number }>("microphone-test-level", (payload) => {
      const raw = Math.max(0, Math.min(1, payload.level ?? 0));
      // Fast-attack / slow-release smoothing so the meter fills left-to-right
      // with the current loudness (the incoming value is already perceptual and
      // backend-smoothed). This drives both the bar length and the active glow.
      const alpha = raw > smoothRef.current ? MIC_ATTACK_ALPHA : MIC_RELEASE_ALPHA;
      const next = raw * alpha + smoothRef.current * (1 - alpha);
      smoothRef.current = next;
      setLevel(next);
      // Peak-hold: jump up instantly, then decay so the loudest recent moment
      // stays marked ahead of the fill.
      peakRef.current = raw >= peakRef.current ? raw : Math.max(next, peakRef.current - MIC_PEAK_DECAY);
      setPeak(peakRef.current);
      const active = nextActive(prevActiveRef.current, next);
      if (active !== prevActiveRef.current) {
        prevActiveRef.current = active;
        setMicActive(active);
      }
    });
    // The modes are switched on by the button handlers — they know which of the
    // two was pressed. What is left here is only an external stop (hiding the
    // window, changing the device): there is one capture stream, and when it
    // ends both modes go out.
    subscribe<unknown>("microphone-test-started", () => { setError(null); });
    subscribe<unknown>("microphone-test-stopped", () => { closePlayback(); setChecking(false); setEcho(false); resetMeter(); setStatus(null); });
    subscribe<AppErrorPayload>("app-error", (payload) => {
      // Permission events are shown by the banner in MainWindow — it has text
      // for the specific permission and a link into the right system settings
      // pane. There must be no such branch here: it labelled every one of them
      // as "no microphone access" with macOS instructions, even though the only
      // source of such events is Accessibility on macOS.
      if (payload?.permission) return;
      if (payload?.message) setError(String(payload.message));
    });
    subscribe<{ message?: string }>("microphone-test-failed", (payload) => {
      closePlayback();
      setChecking(false);
      setEcho(false);
      resetMeter();
      setError(payload?.message ?? t("Тест микрофона не удался"));
      setStoppedStatus(t("Ошибка теста микрофона"));
    });
    return () => {
      closePlayback();
      void invoke("stop_microphone_test").catch(() => {});
      for (const fn of unlisteners) fn();
      if (statusTimerRef.current !== null) {
        window.clearTimeout(statusTimerRef.current);
        statusTimerRef.current = null;
      }
    };
  }, []);

  // The capture is shared by both modes: start is idempotent, and stop happens
  // only when the second mode is off as well — otherwise leaving echo would kill
  // the meter.
  async function startCapture(monitor: boolean) {
    await invoke("start_microphone_test", { microphone: microphone ?? null, monitor });
  }

  // Clear the echo status here and on external stops, including device changes.
  async function stopCapture() {
    closePlayback();
    setChecking(false);
    setEcho(false);
    resetMeter();
    setStatus(null);
    await invoke("stop_microphone_test");
  }

  async function toggleCheck() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (checking) {
        setChecking(false);
        // The capture stays alive for the echo, but the meter belongs to the
        // mode being switched off: without a reset it would keep jumping under
        // a button that is no longer lit.
        if (echo) resetMeter();
        else await stopCapture();
      } else {
        await startCapture(echo);
        setChecking(true);
      }
    } catch (e) {
      await stopCapture().catch(() => {});
      setError(e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  }

  async function toggleEcho() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      if (echo) {
        closePlayback();
        setEcho(false);
        await invoke("set_microphone_test_monitor", { enabled: false });
        if (!checking) await stopCapture();
        setStoppedStatus(t("Эхо выключено"));
      } else {
        // The context is opened before the capture starts: the browser's
        // autoplay gate lifts only inside a click handler.
        const context = new AudioContext();
        playback.current = context;
        await context.resume();
        await startCapture(true);
        setEcho(true);
        setRunningStatus(t("Эхо включено"));
      }
    } catch (e) {
      await stopCapture().catch(() => {});
      setError(e instanceof Error ? e.message : String(e));
    } finally { setBusy(false); }
  }

  async function selectMicrophone(value: string | number | null) {
    await stopCapture();
    await onConfigChanged({ microphone: value });
  }

  return (
    <div>
      <div className="mic-control">
        <CustomSelect className="custom-select--mic" value={selectedValue} options={options.map((option) => ({ ...option, icon: "mic" }))} onOpen={() => void refreshDevices()} onChange={(value) => void selectMicrophone(value).catch((e) => setError(String(e)))}/>
        {/* Headphones come first: the device list is to their left, and echo
            answers the first question about a new microphone ("can I be heard
            at all, and how?"), while the level meter refines the volume. */}
        <Hint text={t("Возвращает ваш голос обратно, чтобы вы слышали себя таким, каким вас слышит программа: шум, хрипы, гулкость комнаты. Только в наушниках: через колонки микрофон поймает сам себя.")}>
          <button className={`mic-test${echo ? " mic-test--active" : ""}`} type="button" disabled={busy} aria-pressed={echo} aria-label={t("Эхо")} onClick={() => void toggleEcho()}><Icon name="headphones" size={14}/></button>
        </Hint>
        <Hint text={t("Индикатор показывает уровень сигнала. Скажите что-нибудь: полоса должна доходить до середины и не упираться в край.")}>
          <button className={`mic-test${checking ? " mic-test--active" : ""}`} type="button" disabled={busy} aria-pressed={checking} aria-label={t("Проверка микрофона")} onClick={() => void toggleCheck()}><Icon name="mic" size={14}/></button>
        </Hint>
        {status && <span className={`mic-status-chip${status.kind === "stopped" ? " mic-status-chip--stop" : ""}`} role="status" aria-live="polite">{status.text}</span>}
        {/* Индикатор показывает только проверка уровня. Поток захвата общий с
            эхом, и события уровня идут при любом из режимов — без этого
            условия одно включённое эхо рисовало бы бегающую полосу, а сама
            кнопка проверки при этом стояла бы погашенной. */}
        <MicMeter level={checking ? level : 0} peak={checking ? peak : 0} active={checking && micActive}/>
      </div>
      {error && <div role="alert" style={{ marginTop: 6, color: "var(--err)", font: "500 11px/1.4 var(--font-sans)" }}>{error}</div>}
    </div>
  );
}

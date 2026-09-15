import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { t, useLocale } from "../i18n";
import { overlayDetail } from "./overlayDetail";
import { OverlayWaveform } from "./OverlayWaveform";
import { useOverlaySession, type OverlaySession } from "./useOverlaySession";

export function OverlayApp() {
  useLocale();
  const session = useOverlaySession();
  const surfaceRef = useRef<HTMLDivElement>(null);
  const { state, streaming, previewText, handleClose, isClosing } = session;
  if (state === null) return null;
  const closeLabel = state === "recording" || state === "processing" || state === "done"
    ? t("Отменить запись") : state === "error" ? t("Закрыть") : t("Отменить");
  return (
    <div data-testid="overlay" data-state={state} data-layout={streaming ? "streaming" : "compact"} className="overlay">
      <div className="overlay-shell">
        <div className="overlay-surface" ref={surfaceRef}>
          <div className="overlay-glow" aria-hidden="true" />
          <div className="overlay-row">
            <TimerBadge session={session} />
            <div className="overlay-detail">
              {state === "recording"
                ? <OverlayWaveform key={session.sessionId} surfaceRef={surfaceRef} />
                : <StateDetail session={session} />}
            </div>
            <button className="overlay-close" aria-label={closeLabel} onClick={handleClose} disabled={isClosing}>
              <Icon name="x" size={15} />
            </button>
          </div>
          {streaming && <PreviewPane text={previewText} />}
        </div>
      </div>
    </div>
  );
}

function useNow(active: boolean) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

function TimerBadge({ session }: { session: OverlaySession }) {
  const now = useNow(session.state === "recording");
  const total = Math.max(0, Math.floor(((session.recordingStoppedAt ?? now) - session.recordingStartedAt) / 1000));
  const duration = session.state === "loading" ? "--:--"
    : `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
  return <div className="overlay-timer"><span className="overlay-dot" /><span>{duration}</span></div>;
}

function StateDetail({ session }: { session: OverlaySession }) {
  const now = useNow(session.state === "done");
  const detail = overlayDetail({
    state: session.state!, pastedLength: session.pastedLength,
    polishingMs: session.decodedAt === null ? 0 : now - session.decodedAt,
    errorText: session.errorText, aiProblem: session.aiProblem,
  });
  if (detail.kind === "waveform") return null;
  if (detail.kind === "progress") {
    return <div className="overlay-progress"><div className="overlay-progress-track"><div /></div>{detail.label && <span>{detail.label}</span>}</div>;
  }
  if ("warning" in detail) {
    return <div className="overlay-result"><div>{detail.text}</div><div>{detail.warning}</div></div>;
  }
  return <div className="overlay-text" title={detail.text}>{detail.text}</div>;
}

function PreviewPane({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  // Hypotheses can rewrite earlier words; show their latest end before paint.
  useLayoutEffect(() => {
    const node = ref.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [text]);
  return <div ref={ref} className={`overlay-preview${text ? "" : " overlay-preview-empty"}`}>
    {text || t("Говорите — текст появится здесь")}
  </div>;
}

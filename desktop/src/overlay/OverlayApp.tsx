import { Hint } from "../components/Hint";
import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { t, useLocale } from "../i18n";
import { overlayPalette } from "./overlayPalette";
import { overlayDetail, recordingClock } from "./overlayDetail";
import { OverlayWaveform } from "./OverlayWaveform";
import { useOverlaySession, type OverlaySession } from "./useOverlaySession";
import type { Recipe, ScenePhase } from "./overlayRecipe";
import { audioLevelSource } from "./levelSource";

// The recipe scene is the constructor's overlay; configs without a recipe keep the forms below.
const OverlayScene = lazy(() => import("./OverlayScene").then((m) => ({ default: m.OverlayScene })));

// A missing animation chunk must not unmount the recording/cancel controls.
const OverlayGlow = lazy(() => import("./OverlayGlow")
  .then((m) => ({ default: m.OverlayGlow }))
  .catch(() => ({ default: GlowFallback })));

function GlowFallback() {
  return <div className="overlay-glow" aria-hidden="true" />;
}

export function OverlayApp() {
  useLocale();
  const session = useOverlaySession();
  const surfaceRef = useRef<HTMLDivElement>(null);
  const { state, layout, previewText, streaming, handleClose, isClosing, config, preferences, hovered, setHovered } = session;
  if (state === null) return null;
  if (preferences.recipe) return <RecipeOverlay session={session} recipe={preferences.recipe}/>;
  const bead = layout === "bead";
  const glow = layout === "glow";
  // The limit countdown shows even with the timer turned off: it is the only
  // warning before the recording stops by itself.
  const timerOn = preferences.show_timer || session.limitAt !== null;
  const showTimer = timerOn && state === "recording";
  const closeLabel = state === "pasted" || state === "error" ? t("Закрыть") : t("Отменить запись");
  return (
    <div data-testid="overlay" data-state={state} data-layout={layout === "pill" ? "compact" : layout} data-size={preferences.size} data-timer={timerOn ? "on" : "off"} data-hovered={hovered ? "true" : "false"} className="overlay" style={config ? overlayPalette(preferences) : undefined}>
      <div className="overlay-shell" onPointerEnter={() => setHovered(true)} onPointerLeave={() => setHovered(false)}>
        <div className="overlay-surface" ref={surfaceRef}>
          {glow ? <>
            <Suspense fallback={null}>
              <OverlayGlow key={session.sessionId} mode={glowMode(state)} size={preferences.size} />
            </Suspense>
            <div className="overlay-composer-body">
              {state === "recording"
                ? streaming ? <PreviewPane text={previewText} /> : null
                : <StateDetail session={session} />}
            </div>
            <div className="overlay-composer-bar">
              {showTimer ? <TimerBadge session={session} /> : <span />}
              <button className="overlay-close" aria-label={closeLabel} onClick={handleClose} disabled={isClosing}>
                <Icon name="x" size={15} />
              </button>
            </div>
          </> : <>
            <div className="overlay-glow" aria-hidden="true" />
            <div className="overlay-row">
              {!bead && <div className="overlay-timer-slot">{showTimer && <TimerBadge session={session} />}</div>}
              <div className="overlay-detail">
                {state === "recording"
                  ? <OverlayWaveform key={session.sessionId} surfaceRef={surfaceRef} circular={bead} />
                  : bead ? <span className="overlay-bead-status" aria-label={beadLabel(state)} role="status" /> : <StateDetail session={session} />}
              </div>
              <button className="overlay-close" aria-label={closeLabel} onClick={handleClose} disabled={isClosing}>
                <Icon name="x" size={15} />
              </button>
            </div>
            {layout === "streaming" && <PreviewPane text={previewText} />}
          </>}
        </div>
      </div>
    </div>
  );
}

function scenePhase(state: NonNullable<OverlaySession["state"]>): ScenePhase {
  if (state === "recording" || state === "pasted" || state === "error") return state;
  return "processing";
}

function RecipeOverlay({ session, recipe }: { session: OverlaySession; recipe: Recipe }) {
  const { state, preferences, config } = session;
  const now = useNow(state === "recording" || state === "done");
  const surfaceRef = useRef<HTMLDivElement>(null);
  if (state === null) return null;
  const clock = recordingClock(session.recordingStartedAt, session.recordingStoppedAt, session.limitAt, now);
  const detail = overlayDetail({
    state, pastedLength: session.pastedLength,
    polishingMs: session.decodedAt === null ? 0 : now - session.decodedAt,
    errorText: session.errorText, aiProblem: session.aiProblem,
  });
  const status = detail.kind === "waveform" ? ""
    : detail.kind === "progress" ? (detail.seconds === undefined ? detail.label : `${detail.label} ${t("{p0} с", { p0: detail.seconds })}`)
    : "warning" in detail ? `${detail.text} · ${detail.warning}` : detail.text;
  const language = (config?.language || "auto").toUpperCase();
  return <div data-testid="overlay" data-state={state} data-layout="recipe" className="overlay" style={config ? overlayPalette(preferences) : undefined}
    onPointerEnter={() => session.setHovered(true)} onPointerLeave={() => session.setHovered(false)}>
    <Suspense fallback={null}>
      <OverlayScene key={session.sessionId} recipe={recipe} size={preferences.size} phase={scenePhase(state)}
        streaming={session.streaming} needsText={state === "error" || (state === "pasted" && !!session.aiProblem)}
        draft={session.previewText} draftPlaceholder={t("Говорите — текст появится здесь")}
        timer={state === "loading" ? "--:--" : clock.text} limited={clock.limited} status={status}
        mode={{ full: config?.model ? `${language} · ${config.model}` : language, short: language }}
        close={{ label: state === "pasted" || state === "error" ? t("Закрыть") : t("Отменить запись"), onClick: session.handleClose, disabled: session.isClosing }}
        hovered={session.hovered} surfaceRef={surfaceRef} source={audioLevelSource}/>
    </Suspense>
  </div>;
}

function glowMode(state: NonNullable<OverlaySession["state"]>): "listen" | "process" | "idle" {
  if (state === "recording") return "listen";
  if (state === "loading" || state === "processing" || state === "done") return "process";
  return "idle";
}

function beadLabel(state: NonNullable<OverlaySession["state"]>) {
  if (state === "loading") return t("Подготавливаю локальную модель");
  if (state === "pasted") return t("Текст вставлен");
  return t("Обрабатываю");
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
  const clock = recordingClock(session.recordingStartedAt, session.recordingStoppedAt, session.limitAt, now);
  const duration = session.state === "loading" ? "--:--" : clock.text;
  const badge = <div className="overlay-timer" data-limit={clock.limited ? "true" : undefined}><span className="overlay-dot" /><span>{duration}</span></div>;
  return clock.limited
    ? <Hint asChild text={t("Запись скоро остановится автоматически и будет распознана")}>{badge}</Hint>
    : badge;
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
    return <div className="overlay-progress"><span>{detail.label}</span>{detail.seconds !== undefined && <span className="overlay-counter">{t("{p0} с", { p0: detail.seconds })}</span>}</div>;
  }
  if ("warning" in detail) {
    return <div className="overlay-result"><div>{detail.text}</div><Hint asChild ifClipped text={detail.warning}><div>{detail.warning}</div></Hint></div>;
  }
  // Both lines are cut to the width of the overlay; the bubble is how the
  // rest of a long error is read, not a second copy of what already fits.
  return <Hint asChild ifClipped text={detail.text}><div className="overlay-text">{detail.text}</div></Hint>;
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

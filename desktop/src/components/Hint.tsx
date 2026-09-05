import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

const GAP = 8;      // зазор между значком и пузырём
const MARGIN = 8;   // минимальный отступ пузыря от края окна

/**
 * Подсказка с пузырём.
 *
 * Без `children` это привычный значок «i». С `children` якорем становится сам
 * контрол: подсказку про него спрашивают, наводя на него, а не на значок
 * рядом — так объяснение, зачем погашен переключатель устройства, живёт на
 * самом переключателе и не занимает строки в вёрстке.
 *
 * Пузырь рендерится порталом в body с position: fixed и координатами,
 * посчитанными от значка: absolute-пузырь внутри страницы обрезался
 * скроллером .main-body, стоило подсказке оказаться у края карточки.
 * Позиция зажимается в окно по горизонтали и переворачивается вниз,
 * если сверху не хватает места.
 */
// Ни размера, ни отступа в пропсах: раньше каждое место вызова задавало их
// само (13/18, 11/14, 10/13, 12/16), и расстояние от подписи до значка нигде
// не совпадало. Геометрия одна и живёт в CSS — эталоном взята строка
// «Горячая клавиша» в настройках.
export function Hint({ text, children }: { text: string; children?: ReactNode }) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const anchor = anchorRef.current?.getBoundingClientRect();
    const bubble = bubbleRef.current?.getBoundingClientRect();
    if (!anchor || !bubble) return;
    const maxLeft = Math.max(MARGIN, window.innerWidth - bubble.width - MARGIN);
    const left = Math.min(Math.max(MARGIN, anchor.left + anchor.width / 2 - bubble.width / 2), maxLeft);
    const above = anchor.top - GAP - bubble.height;
    const top = above >= MARGIN ? above : anchor.bottom + GAP;
    setPos({ left, top });
  }, [open, text]);

  // Пузырь зафиксирован относительно окна и за прокруткой не следует —
  // проще закрыть его, чем пересчитывать на каждый кадр скролла.
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  return (
    <span
      ref={anchorRef}
      className={children ? "hint-anchor" : "hint"}
      // Значок сам по себе фокус не получает — ему его дают, иначе подсказка
      // существует только для мыши. Обёртке вокруг контрола свой фокус не
      // нужен и вреден: он встаёт лишней остановкой перед самим контролом.
      tabIndex={children ? undefined : 0}
      // Имя обёртки заменило бы собой имя контрола внутри, поэтому текст
      // уходит скринридеру отдельной строкой, а не подписью на якоре.
      aria-label={children ? undefined : text}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      {children ?? <Icon name="info" size={11}/>}
      {children && <span className="sr-only">{text}</span>}
      {open && createPortal((
        <div
          ref={bubbleRef}
          className="hint-bubble"
          role="tooltip"
          style={pos ? { left: pos.left, top: pos.top } : { left: 0, top: 0, visibility: "hidden" }}
        >
          {text}
        </div>
      ), document.body)}
    </span>
  );
}

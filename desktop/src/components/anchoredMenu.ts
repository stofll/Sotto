import { useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";

/**
 * Позиционирование выпадающего меню, вынесенного порталом в body.
 *
 * Absolute-меню внутри страницы режет ближайший скроллер: у `.win__main` и у
 * `.modal__body` стоит `overflow: auto`, и список, не поместившийся до
 * нижней границы, обрывался — доскроллить до остатка было нельзя, он просто
 * не рисовался. Fixed-координаты, посчитанные от кнопки, этой границы не
 * знают: меню зажимается в окно и переворачивается вверх, когда снизу места
 * меньше, чем сверху.
 *
 * Пересчёт висит на скролле (в фазе перехвата — прокрутка идёт во внутреннем
 * контейнере, а не в окне) и на resize.
 */
const GAP = 6;
const MARGIN = 8;
const MIN_USABLE = 140;

export type AnchoredMenu = {
  menuRef: RefObject<HTMLDivElement | null>;
  style: CSSProperties;
  /** Меню открылось вверх — вызывающему может понадобиться для стрелки. */
  flipped: boolean;
  /**
   * Меню уже поставлено на место и видимо.
   *
   * До этого оно висит в DOM с `visibility: hidden`, а невидимому элементу
   * нельзя дать фокус: `focus()` внутри такого меню молча ничего не делает.
   * Кому нужен фокус — пусть дождётся этого признака.
   */
  placed: boolean;
};

/**
 * `start` — левый край меню по левому краю кнопки, `end` — правый по правому.
 *
 * Меню действий у кнопки в правом углу карточки обязано открываться внутрь
 * карточки, а не наружу: выровненное по левому краю, оно уезжает за её край
 * и виснет над соседкой.
 */
export type MenuAlign = "start" | "end";

export function useAnchoredMenu(
  open: boolean,
  anchorRef: RefObject<HTMLElement | null>,
  maxHeight = 260,
  align: MenuAlign = "start",
): AnchoredMenu {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [box, setBox] = useState<{ left: number; top: number; minWidth: number; maxHeight: number; flipped: boolean } | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setBox(null);
      return;
    }
    const place = () => {
      const anchor = anchorRef.current?.getBoundingClientRect();
      if (!anchor) return;
      const below = window.innerHeight - anchor.bottom - GAP - MARGIN;
      const above = anchor.top - GAP - MARGIN;
      const flipped = below < Math.min(maxHeight, MIN_USABLE) && above > below;
      const height = Math.max(MIN_USABLE, Math.min(maxHeight, flipped ? above : below));
      // Ширину и высоту берём с уже отрисованного меню: они зависят от
      // содержимого и от CSS, а не от кнопки.
      const width = menuRef.current?.offsetWidth ?? anchor.width;
      // Развёрнутое вверх меню прижимается к кнопке своим низом, поэтому
      // отсчитывать надо его собственную высоту, а не разрешённый максимум:
      // короткое меню из одного пункта тот поднимал на полтораста пикселей
      // выше кнопки — на середину экрана, без всякой связи с ней.
      const shown = Math.min(height, menuRef.current?.offsetHeight ?? height);
      const maxLeft = Math.max(MARGIN, window.innerWidth - width - MARGIN);
      const wanted = align === "end" ? anchor.right - width : anchor.left;
      setBox({
        left: Math.min(Math.max(MARGIN, wanted), maxLeft),
        top: flipped ? Math.max(MARGIN, anchor.top - GAP - shown) : anchor.bottom + GAP,
        minWidth: anchor.width,
        maxHeight: height,
        flipped,
      });
    };
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [open, maxHeight, align, anchorRef]);

  return {
    menuRef,
    flipped: box?.flipped ?? false,
    placed: box !== null,
    // Первый проход — меню уже в DOM, но ещё не измерено: прячем его, чтобы
    // не мигнуть в левом верхнем углу. useLayoutEffect успевает до отрисовки.
    style: box
      ? { left: box.left, top: box.top, minWidth: box.minWidth, maxHeight: box.maxHeight }
      : { left: 0, top: 0, visibility: "hidden" },
  };
}

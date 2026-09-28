import { t } from "../../i18n";
import type { ElementType, Motion, PastedHold, ProcessDraw, ProcessSpeed, RecipeCancel, RecipeStyle, RegionKind, Shell } from "../../overlay/overlayRecipe";
import type { MatrixProcess, MatrixSpeech } from "../../overlay/dotMatrix";

// Names for everything the constructor shows. Built on each call so they
// follow the interface language.

export const shellNames = (): Record<Shell, string> => ({
  pill: t("Пилюля"), card: t("Карточка"), bead: t("Бусина"), stack: t("Стопка"), island: t("Остров"), caps: t("Субтитры"), mini: t("Мини"),
});

export const processNames = (): Record<ProcessDraw, string> => ({
  dots: t("Точки"), arc: t("Дуга"), matrix: t("Матрица"), cursor: t("Курсор"), none: t("Нет"),
});

export const processSpeedNames = (): Record<ProcessSpeed, string> => ({
  slow: t("Медленно"), normal: t("Обычно"), fast: t("Быстро"),
});

export const pastedHoldNames = (): Record<PastedHold, string> => ({
  short: t("Коротко"), normal: t("Обычно"), long: t("Долго"),
});

export const elementNames = (): Record<ElementType, string> => ({
  level: t("Уровень"), timer: t("Таймер"), rec: t("Индикатор"), mode: t("Режим"), draft: t("Черновик"),
});

export const elementNotes = (): Record<ElementType, string> => ({
  level: t("Рисует громкость голоса"), timer: t("Время записи и отсчёт до лимита"), rec: t("Показывает, что идёт запись"),
  draft: t("Текст, который стриминговая модель распознаёт по ходу речи"), mode: t("Язык распознавания и модель"),
});

/** Where the cancel button can sit; the keys are the spots of `CANCEL_SPOTS`. */
export const cancelSpotNames = (): Record<string, string> => ({
  start: t("в начале строки"), end: t("в конце строки"), footL: t("внизу слева"), footR: t("внизу справа"),
  corner: t("в углу"), center: t("по центру"), top: t("сверху"), bottom: t("снизу"),
});

export const cancelDrawNames = (): Record<RecipeCancel["draw"], string> => ({ x: t("Крестик"), stop: t("Квадрат"), text: t("Надпись") });

export const cancelShowNames = (): Record<RecipeCancel["show"], string> => ({ hover: t("При наведении"), always: t("Всегда") });

export const drawNames = (): Record<ElementType, Record<string, string>> => ({
  level: {
    bars: t("Столбики"), wave: t("Волна"), qbars: t("Пиксели"), scope: t("Осциллограф"), ascii: t("Символы"),
    caps: t("Капсулы"), matrix: t("Матрица"), segments: t("Сегменты"), ring: t("Кольцо"), orb: t("Сфера"), beam: t("Свечение по краю"),
  },
  timer: { capsule: t("В капсуле"), plain: t("Цифры"), big: t("Крупные") },
  rec: { dot: t("Точка"), label: t("«Слушаю»"), REC: "REC" },
  mode: { chip: t("Язык и модель"), short: t("Язык") },
  draft: { tail: t("Хвост бледнее"), plain: t("Ровный текст") },
});

export const regionNames = (): Record<string, string> => ({
  start: t("слева"), center: t("центр"), end: t("справа"), below: t("строка текста"),
  body: t("тело"), footL: t("низ слева"), footR: t("низ справа"), edge: t("край"),
  core: t("ядро"), top: t("верх"), bottom: t("низ"), c1: t("чип 1"), c2: t("чип 2"), lines: t("строки"),
});

export const kindPlaces = (): Record<RegionKind, string> => ({
  small: t("места по бокам"), wide: t("широкий центр"), text: t("строку текста"),
  square: t("ядро бусины"), tall: t("стопку"), edge: t("край карточки"),
});

export const styleNames = (): { [K in keyof RecipeStyle]: [string, Record<RecipeStyle[K], string>] } => ({
  radius: [t("Скругление"), { round: t("Круглое"), soft: t("Мягкое"), sharp: t("Острое") }],
  stroke: [t("Обводка"), { none: t("Нет"), hair: t("Тонкая"), rim: t("Обод") }],
  fill: [t("Заливка"), { palette: t("Палитра"), light: t("Светлая"), black: t("Чёрная"), none: t("Нет") }],
  glow: [t("Свечение от голоса"), { "0": t("Нет"), "1": t("Тихое"), "2": t("Яркое") }],
  font: [t("Шрифт"), { sans: t("Обычный"), mono: t("Моно") }],
});

export const motionNames = (): Record<Motion, [string, string]> => ({
  quiet: [t("Тихо"), t("Мгновенно, меняется только цвет")],
  soft: [t("Мягко"), t("Плавно, с лёгким размытием")],
  spring: [t("Пружина"), t("С перелётом, как Dynamic Island")],
  pixel: [t("Ступени"), t("Три шага, без размытия")],
});

export const speechPatternNames = (): Record<MatrixSpeech, [string, string]> => ({
  rings: [t("Круги"), t("громкость расходится от центра")],
  wave: [t("Волна"), t("голос бежит по сетке")],
  ripple: [t("Рябь"), t("каждый слог пускает кольцо")],
  field: [t("Поле"), t("живой шум, гуще от громкости")],
});

export const processPatternNames = (): Record<MatrixProcess, [string, string]> => ({
  perimeter: [t("Периметр"), t("змейка бежит по краю")],
  scan: [t("Скан"), t("полоса ходит туда и обратно")],
  spiral: [t("Спираль"), t("сетка заполняется и гаснет")],
  sonar: [t("Сонар"), t("кольца расходятся от центра")],
});

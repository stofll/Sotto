import { getLocale } from "../../desktop/src/i18n";

// A developer tool: its strings stay out of the application's dictionary and
// bundle. As in the app, the Russian text is the key and the fallback.
export const en: Record<string, string> = {
  "[неразборчиво]": "[unclear]",
  "Глагол": "Verb",
  "Первый термин": "First term",
  "Второй термин": "Second term",
  "Проверка по аудио": "Audio review",
  "Полный текст, короткие записи, только нужные исправления.": "Full transcripts, short recordings, only the corrections needed.",
  "Язык интерфейса": "Interface language",
  "Светлая тема": "Light theme",
  "Тёмная тема": "Dark theme",
  "Не удалось загрузить записи. Проверьте, что локальный сервер работает, и обновите страницу.": "Could not load recordings. Check that the local server is running, then reload.",
  "Загрузка…": "Loading…",
  "Обновить страницу": "Reload page",
  "В этой партии пока нет записей.": "No recordings in this batch yet.",
  "Партия {batch} · {done} из {total} проверено": "Batch {batch} · {done} of {total} reviewed",
  "Аудио остаётся на этом компьютере": "Audio stays on this computer",
  "Записи": "Recordings",
  "Запись {n}": "Recording {n}",
  "Запись {n} · {seconds} с": "Recording {n} · {seconds} s",
  "Проверено": "Reviewed",
  "Ожидает проверки": "Awaiting review",
  "Аудиозапись": "Audio recording",
  "Слушать с начала": "Play from start",
  "Скорость": "Speed",
  "Открыть WAV": "Open WAV",
  "Не удалось воспроизвести аудио. Откройте WAV или обновите страницу.": "Could not play audio. Open the WAV or reload.",
  "Путь к файлу": "File path",
  "Полный текст записи": "Full transcript",
  "Выбранные варианты уже подставлены. Проверьте и остальные слова по аудио.": "Your choices are inserted. Check the remaining words against the audio too.",
  "Исходный текст Ultra": "Original Ultra transcript",
  "Места для проверки": "Places to review",
  "Выберите вариант по аудио или напишите свой. Изменения сразу появятся в полном тексте.": "Choose an option against the audio or write your own. Changes appear in the full transcript immediately.",
  "Свой вариант": "Your own wording",
  "Свой вариант для {id}": "Your wording for {id}",
  "Напишите только этот фрагмент": "Write just this phrase",
  "Неразборчиво": "Unclear",
  "Есть другие исправления в полном тексте": "Other words in the full transcript need correction",
  "Исправьте нужные слова ниже. Сохранится этот полный текст; повторения оставьте как в аудио. Пунктуация не требуется.": "Correct the words below. This full text will be saved; keep repetitions as spoken. Punctuation is optional.",
  "Исправленный полный текст": "Corrected full transcript",
  "Чтобы снова выбирать варианты, снимите галочку свободной правки. Полный текст вернётся к выбранным вариантам.": "To choose options again, uncheck free editing. The full text will return to your selected options.",
  "Сохранение означает, что все слова в полном тексте проверены по аудио.": "Saving confirms that all words in the full transcript have been checked against the audio.",
  "Сохранение…": "Saving…",
  "Сохранить проверенную запись": "Save reviewed recording",
  "Сохранить только фрагменты": "Save phrases only",
  "Следующая запись": "Next recording",
  "Сохранено. Полный текст добавлен в эталон для метрик.": "Saved. The full transcript is now a reference for metrics.",
  "Фрагменты сохранены. Полный текст пока не считается эталоном.": "Phrases saved. The full transcript is not yet an approved reference.",
  "Не удалось сохранить. Черновик сохранён в браузере; обновите страницу и повторите.": "Could not save. Your draft stays in this browser; reload and retry.",
  "Для полного эталона нужно разобрать все отмеченные места. Можно сохранить только фрагменты.": "Resolve every marked phrase to approve the full transcript. You can save just the phrases.",
  "Слова — как в аудио. Предложения по чистовому тексту проверяем отдельно.": "Keep the words as spoken. Cleaned wording is reviewed separately.",
};

/** Evaluate at render time; the root re-renders through `useLocale`. */
export function t(key: string, params: Record<string, string | number> = {}): string {
  const text = getLocale() === "en" ? en[key] ?? key : key;
  return text.replace(/\{(\w+)\}/g, (match, name: string) => name in params ? String(params[name]) : match);
}

/**
 * Copy for the on-site privacy page.
 *
 * It summarises docs/privacy.md rather than reproducing it: the repository
 * holds the authoritative document and this page links to it. Change the
 * document first, then this summary.
 */
export interface PrivacyPage {
  title: string;
  lede: string;
  updated: string;
  sections: { heading: string; paragraphs: string[]; list?: string[] }[];
  fullDocument: string;
  backHome: string;
}

export const privacyEn: PrivacyPage = {
  title: 'Privacy',
  lede: 'What Sotto does with your voice, what it never touches, and what this website does. The short version: recognition runs on your computer unless you deliberately connect a cloud provider.',
  updated: 'Summarised from the repository document on 1 October 2026.',
  sections: [
    {
      heading: 'Your speech stays on your computer',
      paragraphs: [
        'Local speech recognition is the default. You download a model once, and from then on dictation works without an internet connection. There is no account, no sign-in, and no per-minute quota on local dictation.',
        'Nothing about a local dictation leaves your machine: not the audio, not the transcript, not the text that gets typed into your application.',
      ],
    },
    {
      heading: 'Network requests',
      paragraphs: ['These features contact external services:'],
      list: [
        'A cloud speech provider you connect yourself receives the audio you dictate, and only while you use it.',
        'Cloud text processing you configure yourself receives the text to be reformatted, never the audio.',
        'The model downloader contacts the model hosting endpoint when you download a model.',
        'Installed release builds contact GitHub Releases to check for updates at startup and when you request a check, and to fetch release notes. An update is downloaded and installed only after you request installation.',
        'If you turn on Help → Updates → Receive beta builds, the update check also reads the public list of GitHub releases to find newer betas.',
      ],
    },
    {
      heading: 'Product telemetry',
      paragraphs: [
        'Telemetry is off until you allow it. The first-launch introduction offers it, and if you skip that, the app asks once after a successful dictation; before it is turned on you see the list of events, and Settings → Advanced → Telemetry changes your answer later. It sends a small, fixed list of de-identified usage events to PostHog Cloud EU, keyed by a random installation identifier that is not derived from your account, username, hostname, MAC address, file paths, or any hardware fingerprint.',
        'It never sends transcripts, formatted output, prompts, clipboard contents, audio, filenames, filesystem paths, usernames, hostnames, API keys, provider responses, microphone names, details of the focused window, or raw error text.',
        'Switching it off stops further capture and delivery immediately. It does not retract events already delivered.',
      ],
    },
    {
      heading: 'What is kept on your computer',
      paragraphs: [
        'Dictation history, settings, the telemetry outbox, and optional audio recordings are stored locally. Saving audio recordings is a separate setting you have to turn on yourself.',
        'Model speed indicators use reference measurements bundled with the app. They do not record the speed of your own dictations or model loads. Older builds kept such measurements locally; newer builds remove them on first launch.',
      ],
    },
    {
      heading: 'Reports you send yourself',
      paragraphs: [
        'The Help page can open a GitHub issue template in your browser with a technical summary you have reviewed. Opening that page hands the summary to GitHub, and it may appear in your browser history; publishing it is a separate, deliberate action on your side.',
        'Sanitized logs are prepared locally and saved where you choose. Sotto never uploads them. You attach the file yourself if you decide to.',
      ],
    },
    {
      heading: 'This website',
      paragraphs: [
        'This page and the landing page carry no analytics, no tracking pixels, no cookies, and no third-party scripts. Fonts are served from this domain rather than a font CDN.',
        'The only outbound request the site can make is to the GitHub releases API, and only after you click a download button, so that the dialog can offer a direct link to the right installer. If you never click it, the site talks to nothing.',
      ],
    },
  ],
  fullDocument: 'Read the full privacy document',
  backHome: 'Back to the home page',
};

export const privacyRu: PrivacyPage = {
  title: 'Приватность',
  lede: 'Что Sotto делает с вашим голосом, чего не касается никогда и как устроен этот сайт. Коротко: распознавание работает на вашем компьютере, пока вы сами не подключите облачного провайдера.',
  updated: 'Составлено по документу из репозитория 1 октября 2026 года.',
  sections: [
    {
      heading: 'Речь остаётся на вашем компьютере',
      paragraphs: [
        'Локальное распознавание работает по умолчанию. Модель скачивается один раз, дальше диктовка работает без подключения к сети. Ни аккаунта, ни входа, ни лимита по минутам на локальную диктовку.',
        'С локальной диктовки не уходит ничего: ни аудио, ни расшифровка, ни текст, который подставляется в ваше приложение.',
      ],
    },
    {
      heading: 'Обращения к сети',
      paragraphs: ['Эти функции обращаются к внешним сервисам:'],
      list: [
        'Облачный провайдер распознавания, которого вы подключили сами, получает надиктованное аудио, и только пока вы им пользуетесь.',
        'Облачная обработка текста, настроенная вами, получает текст для переформатирования, но не аудио.',
        'Загрузчик моделей обращается к хранилищу моделей, когда вы скачиваете модель.',
        'Установленная релизная версия обращается к GitHub Releases для проверки обновлений при запуске и по вашему запросу, а также за описанием изменений. Загрузка и установка обновления начинаются только после вашего запроса на установку.',
        'Если включить «Справка → Обновления → Получать бета-сборки», проверка обновлений также читает публичный список релизов на GitHub, чтобы найти новые бета-версии.',
      ],
    },
    {
      heading: 'Продуктовая телеметрия',
      paragraphs: [
        'Телеметрия выключена, пока вы её не разрешите. Её предлагает введение при первом запуске, а если вы его пропустили, приложение один раз спросит после успешной диктовки. Перед включением показывается список событий; изменить ответ можно в «Настройки → Дополнительно → Телеметрия». Она отправляет небольшой фиксированный набор обезличенных событий в PostHog Cloud EU. В качестве ключа используется случайный идентификатор установки. Он не выводится из аккаунта, имени пользователя, имени компьютера, MAC-адреса, путей к файлам или характеристик железа.',
        'В неё никогда не попадают расшифровки, отформатированный текст, промпты, содержимое буфера обмена, аудио, имена файлов, пути, имена пользователя и компьютера, ключи API, ответы провайдеров, названия микрофонов, сведения об активном окне и текст ошибок.',
        'Отключение немедленно останавливает сбор и отправку. Уже доставленные события оно не отзывает.',
      ],
    },
    {
      heading: 'Что хранится на вашем компьютере',
      paragraphs: [
        'История диктовок, настройки, очередь телеметрии и необязательные аудиозаписи хранятся локально. Сохранение аудиозаписей нужно включить отдельно.',
        'Индикаторы скорости моделей используют эталонные замеры, включённые в приложение. Скорость ваших диктовок и загрузок моделей не записывается. Старые версии хранили такие замеры локально; новые удаляют их при первом запуске.',
      ],
    },
    {
      heading: 'Отчёты, которые вы отправляете сами',
      paragraphs: [
        'Страница помощи может открыть в браузере шаблон issue на GitHub с технической сводкой, которую вы перед этим просмотрели. При открытии страницы сводка уходит к GitHub и может остаться в истории браузера. Публиковать issue или нет, решаете вы.',
        'Очищенные логи готовятся локально и сохраняются туда, куда вы укажете. Sotto их никуда не загружает. Файл прикладываете вы сами, если решите это сделать.',
      ],
    },
    {
      heading: 'Этот сайт',
      paragraphs: [
        'На этой и на главной странице нет аналитики, трекинговых пикселей, кук и сторонних скриптов. Шрифты отдаются с этого же домена, а не с шрифтового CDN.',
        'Сайт может сделать только один исходящий запрос: к API релизов GitHub, и только после нажатия на кнопку загрузки, чтобы диалог предложил прямую ссылку на нужный установщик. Если не нажимать, сайт не обращается никуда.',
      ],
    },
  ],
  fullDocument: 'Прочитать полный документ о приватности',
  backHome: 'Вернуться на главную',
};

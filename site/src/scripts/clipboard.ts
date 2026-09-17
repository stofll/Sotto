import { format, type Behaviour, type Runtime } from './runtime';

/** Copying that works over HTTPS and from a local file, with an honest failure state. */
export const initClipboard = ({ query, signal, text, later, strings }: Runtime, repo: string): Behaviour => {
  const button = query<HTMLButtonElement>('[data-copy-clone]');
  const status = query('[data-copy-status]');

  button?.addEventListener(
    'click',
    async () => {
      const command = `git clone ${repo}.git`;
      let copied = false;

      try {
        if (navigator.clipboard && window.isSecureContext) {
          await navigator.clipboard.writeText(command);
          copied = true;
        }
      } catch {
        /* Falls back to a selected text field below. */
      }

      if (!copied && !signal.aborted) {
        const input = document.createElement('textarea');
        input.value = command;
        input.setAttribute('readonly', '');
        input.style.cssText = 'position:fixed;left:-9999px;top:0;opacity:0';
        document.body.append(input);
        input.select();
        try {
          copied = document.execCommand('copy');
        } catch {
          copied = false;
        }
        input.remove();
        button.focus();
      }

      if (signal.aborted) return;

      text(status, copied ? strings.copy.copied : format(strings.copy.manual, { command }));
      status?.classList.remove('sr-only');
      button.setAttribute('aria-label', copied ? strings.copy.copiedAria : strings.copy.aria);
      button.title = copied ? strings.copy.copiedTitle : strings.copy.fallbackTitle;

      later(
        () => {
          status?.classList.add('sr-only');
          button.setAttribute('aria-label', strings.copy.aria);
          button.title = strings.copy.aria;
        },
        copied ? 2400 : 9000,
      );
    },
    { signal },
  );

  return {};
};

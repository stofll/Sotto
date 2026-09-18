import { format, type Behaviour, type Runtime } from './runtime';

interface ReleaseAsset {
  name?: unknown;
  browser_download_url?: unknown;
}

interface Release {
  tag_name?: unknown;
  assets: ReleaseAsset[];
}

interface Config {
  api: string;
  assetPrefix: string;
  releases: string;
}

/**
 * Platform-aware download dialog. The GitHub API is contacted only after an
 * explicit click, and every path falls back to the real release page.
 */
export const initDownload = (runtime: Runtime, config: Config): Behaviour => {
  const { query, all, signal, text, later, clear, strings } = runtime;
  const s = strings.dialog;

  const dialog = query<HTMLDialogElement>('[data-download-dialog]');
  const target = query<HTMLAnchorElement>('[data-download-target]');
  const status = query('[data-download-resolution]');
  const installSteps = query('[data-install-steps]');

  let downloadAbort: AbortController | null = null;
  let cachedRelease: Release | null = null;
  let sequence = 0;
  let returnFocus: HTMLElement | null = null;

  /** Only ever links to an asset served from this repository's release path. */
  const officialAssetURL = (value: string): string | null => {
    try {
      const url = new URL(value);
      return url.protocol === 'https:' &&
        url.hostname === 'github.com' &&
        url.pathname.toLowerCase().startsWith(config.assetPrefix)
        ? url.href
        : null;
    } catch {
      return null;
    }
  };

  const getAsset = (release: Release, platform: 'macos' | 'windows') =>
    release.assets.find((asset) => {
      if (typeof asset.name !== 'string' || typeof asset.browser_download_url !== 'string') return false;
      if (!officialAssetURL(asset.browser_download_url)) return false;
      return platform === 'macos'
        ? /\.dmg$/i.test(asset.name) && /(aarch64|arm64)/i.test(asset.name)
        : /\.exe$/i.test(asset.name) && /(x64|x86_64)/i.test(asset.name);
    });

  const resolve = async (platform: 'macos' | 'windows', current: number) => {
    downloadAbort?.abort();
    const controller = new AbortController();
    downloadAbort = controller;
    const timeout = later(() => controller.abort(), 7000);

    try {
      let release = cachedRelease;
      if (!release) {
        const response = await fetch(config.api, {
          signal: controller.signal,
          headers: { Accept: 'application/vnd.github+json' },
          credentials: 'omit',
          referrerPolicy: 'no-referrer',
        });
        if (!response.ok) throw new Error(`GitHub returned ${response.status}`);

        const data: unknown = await response.json();
        if (!data || typeof data !== 'object' || !('assets' in data) || !Array.isArray(data.assets)) {
          throw new Error('Unrecognized release response');
        }
        release = data as Release;
        cachedRelease = release;
      }

      if (signal.aborted || current !== sequence || !dialog?.open) return;

      const asset = getAsset(release, platform);
      const url = asset && typeof asset.browser_download_url === 'string'
        ? officialAssetURL(asset.browser_download_url)
        : null;

      if (target && url) {
        target.href = url;
        // Text nodes only: remote release metadata is never interpreted as markup.
        target.replaceChildren(document.createTextNode(platform === 'macos' ? s.directMac : s.directWindows));
        const version = typeof release.tag_name === 'string' ? release.tag_name.slice(0, 40) : s.latestRelease;
        text(status, format(s.resolved, { version }));
      } else {
        text(status, s.choosePlatform);
      }
    } catch {
      if (!signal.aborted && current === sequence && dialog?.open) text(status, s.lookupFailed);
    } finally {
      clear(timeout);
    }
  };

  all<HTMLAnchorElement>('[data-download]').forEach((link) =>
    link.addEventListener(
      'click',
      (event) => {
        if (
          event.ctrlKey || event.metaKey || event.shiftKey || event.altKey ||
          event.button !== 0 || !dialog || typeof dialog.showModal !== 'function'
        ) {
          return;
        }

        event.preventDefault();
        const platform = link.dataset.download === 'windows' ? 'windows' : 'macos';
        returnFocus = link;
        sequence += 1;

        text(query('#download-dialog-heading'), platform === 'macos' ? s.titleMac : s.titleWindows);
        text(query('[data-download-description]'), platform === 'macos' ? s.descriptionMac : s.descriptionWindows);

        const steps = platform === 'macos' ? s.stepsMac : s.stepsWindows;
        installSteps?.replaceChildren(
          ...steps.map((step) => {
            const item = document.createElement('li');
            item.textContent = step;
            return item;
          }),
        );

        if (target) {
          target.href = config.releases;
          target.textContent = s.openRelease;
        }

        text(status, s.checking);
        if (!dialog.open) dialog.showModal();
        document.body.classList.add('dialog-open');
        void resolve(platform, sequence);
      },
      { signal },
    ),
  );

  query('[data-dialog-close]')?.addEventListener('click', () => dialog?.close(), { signal });

  dialog?.addEventListener(
    'click',
    (event) => {
      const rect = dialog.getBoundingClientRect();
      if (
        event.target === dialog &&
        (event.clientX < rect.left || event.clientX > rect.right ||
          event.clientY < rect.top || event.clientY > rect.bottom)
      ) {
        dialog.close();
      }
    },
    { signal },
  );

  dialog?.addEventListener(
    'close',
    () => {
      downloadAbort?.abort();
      document.body.classList.remove('dialog-open');
      returnFocus?.focus({ preventScroll: true });
    },
    { signal },
  );

  return {
    destroy: () => {
      downloadAbort?.abort();
      if (dialog?.open) dialog.close();
      document.body.classList.remove('dialog-open');
    },
  };
};

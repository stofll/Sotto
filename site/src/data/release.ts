import { RELEASES_API } from './product';

/**
 * The current release, resolved once at build time.
 *
 * Deliberately not fetched in the browser. The page contacts GitHub only after
 * a reader clicks a download button, and a version badge is not a good reason
 * to give that up on a site whose whole argument is that it does not phone
 * home. Baking it in costs nothing at runtime and stays accurate as long as the
 * site is rebuilt on release.
 *
 * A failed lookup is not a build failure: the badge simply does not render.
 */
export interface Release {
  version: string;
  published: string;
}

let cached: Release | null | undefined;

export const latestRelease = async (): Promise<Release | null> => {
  if (cached !== undefined) return cached;

  try {
    const response = await fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: AbortSignal.timeout(6000),
    });
    if (!response.ok) throw new Error(`GitHub returned ${response.status}`);

    const data: unknown = await response.json();
    if (!data || typeof data !== 'object') throw new Error('Unrecognized release response');

    const { tag_name: tag, published_at: published } = data as Record<string, unknown>;
    cached =
      typeof tag === 'string' && tag.length > 0 && tag.length <= 40
        ? { version: tag, published: typeof published === 'string' ? published : '' }
        : null;
  } catch (error) {
    console.warn(`[release] could not resolve the latest version: ${(error as Error).message}`);
    cached = null;
  }

  return cached;
};

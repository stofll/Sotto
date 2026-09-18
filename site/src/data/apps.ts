/**
 * Applications shown in the "works where you work" strip. Sotto types into the
 * focused field of whatever app is in front, so the only claim this list makes
 * is that these are ordinary desktop apps with a text field.
 *
 * Marks come from simple-icons (CC0). Apps whose owners have withdrawn their
 * logo from that set — Slack, VS Code, the Microsoft Office apps, Apple Notes —
 * are left out rather than drawn by hand: a wrong logo is worse than no row.
 */
import {
  siClaude,
  siCursor,
  siDiscord,
  siEvernote,
  siFigma,
  siFirefox,
  siGithub,
  siGoogledocs,
  siGooglechrome,
  siJetbrains,
  siJira,
  siLinear,
  siLogseq,
  siNeovim,
  siNotion,
  siObsidian,
  siSignal,
  siTelegram,
  siTrello,
  siWarp,
  siWhatsapp,
  siZoom,
} from 'simple-icons';

export interface App {
  /** Stable id, used for the sprite symbol and as the React-style key. */
  id: string;
  /** Brand name. Not translated: it is what the app calls itself. */
  name: string;
  /** Single-path outline, 24x24 viewBox. */
  path: string;
}

const app = ({ title, path }: { title: string; path: string }, name?: string): App => ({
  id: (name ?? title).toLowerCase().replace(/[^a-z0-9]+/g, '-'),
  name: name ?? title,
  path,
});

/** Mixed on purpose: browsers, editors, chat and planning tools alternate. */
export const apps: App[] = [
  app(siGooglechrome, 'Chrome'),
  app(siNotion),
  app(siTelegram),
  app(siCursor),
  app(siFigma),
  app(siWhatsapp),
  app(siObsidian),
  app(siLinear),
  app(siFirefox),
  app(siGoogledocs, 'Docs'),
  app(siDiscord),
  app(siJetbrains),
  app(siTrello),
  app(siSignal),
  app(siGithub),
  app(siLogseq),
  app(siZoom),
  app(siNeovim),
  app(siJira),
  app(siEvernote),
  app(siWarp),
  app(siClaude),
];

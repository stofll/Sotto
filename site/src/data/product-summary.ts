import { LINKS, llmProviders, models } from './product';
import { format, localePath, useTranslations, type Locale } from '../i18n';

/** Optional agent entry point, generated from the same facts as the HTML. */
export const productSummary = (locale: Locale, site: URL): string => {
  const t = useTranslations(locale);
  const total = models.length;
  const url = (path: string) => new URL(path, site).href;
  const home = url(localePath(locale));

  return [
    '# Sotto',
    '',
    `> ${t.meta.description}`,
    '',
    t.hero.subtitle,
    '',
    `## ${t.faq.factsLabel}`,
    '',
    ...t.faq.facts.map(([label, value]) => `- ${label}: ${format(value, { total })}`),
    `- ${t.features.custom.title}: ${llmProviders.join(', ')}`,
    '',
    `## ${t.features.eyebrow}`,
    '',
    ...[t.features.anywhere, t.features.offline, t.features.live, t.features.custom, t.features.files, t.features.history]
      .map((card) => `- ${card.title}: ${card.text}`),
    '',
    `## ${t.models.eyebrow}`,
    '',
    ...models.map((model) => {
      const traits = [model.streaming && t.models.streaming, model.punctuation && t.models.punctuation].filter(Boolean);
      return `- ${model.name} (${model.engine}): ${model.languages.join(', ')}, ${model.size}${traits.length ? `, ${traits.join(', ')}` : ''}`;
    }),
    '',
    `## ${t.start.title}`,
    '',
    ...t.start.steps.map((step, index) => `${index + 1}. ${step.title}: ${step.text}`),
    '',
    t.start.note,
    '',
    `## ${t.faq.eyebrow}`,
    '',
    ...t.faq.items.flatMap((item) => [`### [${item.question}](${home}#faq-${item.id})`, '', item.answer, '']),
    `## ${t.footer.docs}`,
    '',
    `- [Sotto — English](${url(localePath('en'))})`,
    `- [Sotto — Русский](${url(localePath('ru'))})`,
    `- [English — llms.txt](${url('/llms.txt')})`,
    `- [Русский — llms.txt](${url('/ru/llms.txt')})`,
    `- [${t.nav.download}](${LINKS.releases})`,
    `- [GitHub](${LINKS.github})`,
    `- [${t.footer.docs}](${LINKS.docs})`,
    `- [${t.models.eyebrow}](${LINKS.models})`,
    `- [${t.dialog.noticeLink}](${LINKS.install})`,
    `- [${t.privacy.link}](${url(localePath(locale, 'privacy'))})`,
    '',
  ].join('\n');
};

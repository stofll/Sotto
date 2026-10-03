import assert from 'node:assert/strict';
import { glob, readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { en } from '../src/i18n/en.ts';
import { ru } from '../src/i18n/ru.ts';
import { models } from '../src/data/product.ts';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));
const origin = 'https://sotto.today';
const read = (file) => readFile(join(dist, file), 'utf8');
// These checks inspect Astro's generated markup, not arbitrary user HTML.
const tags = (html, name) => [...html.matchAll(new RegExp(`<${name}\\b[^>]*>`, 'g'))].map(([tag]) =>
  Object.fromEntries([...tag.matchAll(/([\w:-]+)="([^"]*)"/g)].map(([, key, value]) => [key, value])));
const schemas = (html) => [...html.matchAll(/<script\b[^>]*type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/g)]
  .flatMap(([, json]) => { const data = JSON.parse(json); return data['@graph'] ?? [data]; });
const decode = (value) => value.replace(/&(?:amp|lt|gt|quot|apos|#39|#x27);/g,
  (entity) => ({ '&amp;': '&', '&lt;': '<', '&gt;': '>', '&quot;': '"', '&apos;': "'", '&#39;': "'", '&#x27;': "'" })[entity]);
const fileFor = (url) => `${decodeURIComponent(url.pathname).slice(1)}${url.pathname.endsWith('/') ? 'index.html' : ''}`;
const pages = [];
for await (const file of glob('**/*.html', { cwd: dist })) pages.push(file);
assert.ok(pages.length >= 6, 'Run pnpm build before pnpm test');

test('indexable pages have canonical URLs, reciprocal locale links and social images', async () => {
  const canonicals = [];
  for (const file of pages) {
    const html = await read(file);
    const meta = tags(html, 'meta');
    const links = tags(html, 'link');
    const canonical = links.filter((link) => link.rel === 'canonical');
    const robots = meta.find((tag) => tag.name === 'robots')?.content ?? '';
    assert.equal((html.match(/<h1\b/g) ?? []).length, 1, `${file}: one primary heading`);
    if (robots.includes('noindex')) {
      assert.ok(file.endsWith('404.html'), `${file}: unexpectedly excluded from search`);
      assert.equal(canonical.length, 0);
      assert.equal(schemas(html).length, 0);
      continue;
    }
    assert.ok(!file.endsWith('404.html'), `${file}: error pages must stay out of search`);
    assert.equal(canonical.length, 1, `${file}: one canonical`);
    const url = new URL(canonical[0].href);
    assert.equal(url.origin, origin);
    assert.equal(fileFor(url).replaceAll('/', '\\'), file.replaceAll('/', '\\'));
    canonicals.push(url.href);
    assert.ok(robots.includes('max-image-preview:large'));
    for (const language of ['en', 'ru', 'x-default']) {
      const alternate = links.find((link) => link.hreflang === language);
      assert.ok(alternate, `${file}: missing ${language} alternate`);
      const translated = await read(fileFor(new URL(alternate.href)));
      assert.ok(tags(translated, 'link').some((link) => link.hreflang && link.href === url.href), `${file}: missing return alternate`);
    }
    assert.equal(meta.find((tag) => tag.property === 'og:url')?.content, url.href);
    const image = new URL(meta.find((tag) => tag.property === 'og:image')?.content);
    assert.equal(image.origin, origin);
    assert.ok((await stat(join(dist, image.pathname.slice(1)))).size > 0);
    assert.ok(meta.find((tag) => tag.name === 'description')?.content.length > 0);
  }
  const sitemap = await read('sitemap-0.xml');
  const listed = [...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map(([, url]) => url);
  assert.deepEqual(listed.sort(), canonicals.sort(), 'Sitemap must contain only canonical, indexable HTML pages');
  assert.match(await read('robots.txt'), /User-agent: \*\s+Allow: \//);
  assert.ok((await read('robots.txt')).includes(`${origin}/sitemap-index.xml`));
});

for (const [locale, t] of Object.entries({ en, ru })) {
  const prefix = locale === 'en' ? '' : 'ru/';
  test(`${locale}: application, website, page and FAQ describe the visible product`, async () => {
    const html = await read(`${prefix}index.html`);
    const graph = schemas(html);
    const website = graph.find((node) => node['@type'] === 'WebSite');
    const page = graph.find((node) => node['@type'] === 'WebPage');
    const app = graph.find((node) => node['@type'] === 'SoftwareApplication');
    const faq = graph.find((node) => node['@type'] === 'FAQPage');
    assert.equal(website?.url, `${origin}/`);
    assert.equal(website?.name, 'Sotto');
    assert.equal(page?.isPartOf['@id'], website['@id']);
    assert.equal(page?.mainEntity['@id'], app?.['@id']);
    assert.equal(app?.['@id'], `${origin}/#application`);
    assert.equal(page?.hasPart['@id'], faq?.['@id']);
    assert.equal(faq?.isPartOf['@id'], page['@id']);
    assert.equal(page.inLanguage, locale);
    assert.equal(Number(app.offers.price), 0);
    assert.ok(!app.aggregateRating && !app.review, 'Do not invent reviews for a rich result');
    assert.ok(!('codeRepository' in app), 'codeRepository belongs to SoftwareSourceCode');

    const questions = [...html.matchAll(/<details\b[^>]*id="(faq-[^"]+)"[^>]*>\s*<summary>([\s\S]*?)<\/summary>\s*<p>([\s\S]*?)<\/p>/g)];
    assert.equal(questions.length, t.faq.items.length);
    assert.equal(new Set(questions.map(([, id]) => id)).size, questions.length);
    assert.equal(faq.mainEntity.length, questions.length);
    for (const [, id, question, answer] of questions) {
      const entry = faq.mainEntity.find((item) => item.url === `${origin}/${prefix}#${id}`);
      assert.equal(entry?.name, decode(question));
      assert.equal(entry?.acceptedAnswer.text, decode(answer));
    }

    const summary = await read(`${prefix}llms.txt`);
    assert.ok(tags(html, 'link').some((link) => link.type === 'text/plain' && link.href === `${origin}/${prefix}llms.txt`));
    for (const item of t.faq.items) {
      assert.ok(summary.includes(item.answer), `${item.id}: text summary must use the visible answer`);
      assert.ok(summary.includes(`${origin}/${prefix}#faq-${item.id}`));
    }
    for (const model of models) {
      const line = summary.split('\n').find((line) => line.startsWith(`- ${model.name} (${model.engine}):`));
      assert.ok(line?.includes(model.size), `${model.name}: summary size differs from the catalog`);
    }
    assert.ok(summary.includes(`- Models: ${models.length}`) || summary.includes(`Во встроенном каталоге: ${models.length}`));
  });
}

test('FAQ translations preserve the same link identifiers', () => {
  assert.deepEqual(en.faq.items.map((item) => item.id), ru.faq.items.map((item) => item.id));
});

test('conventional crawler paths redirect to files the build emits', async () => {
  const rules = (await read('_redirects')).split('\n').filter((line) => line.trim() && !line.startsWith('#'))
    .map((line) => line.trim().split(/\s+/));
  assert.deepEqual(rules.map(([from]) => from).sort(), ['/favicon.ico', '/sitemap.xml']);
  for (const [from, to, status] of rules) {
    assert.equal(status, '301', `${from}: permanent redirect`);
    await assert.rejects(stat(join(dist, from.slice(1))), `${from}: a real file would shadow the redirect`);
    assert.ok((await stat(join(dist, to.slice(1)))).size > 0, `${from}: target ${to} is missing from the build`);
  }
});

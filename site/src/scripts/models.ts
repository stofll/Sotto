import { format, type Behaviour, type Runtime } from './runtime';

/**
 * Narrows the model catalog by language group and by streaming support. The
 * full list is rendered on the server, so without JavaScript nothing is hidden.
 */
export const initModels = ({ query, all, signal, text, strings }: Runtime): Behaviour => {
  const cards = all('.model-card');
  const chips = all<HTMLButtonElement>('[data-model-filters] [data-filter]');
  const streamingToggle = query<HTMLButtonElement>('[data-streaming-filter]');
  const count = query('[data-model-count]');
  const empty = query('[data-model-empty]');
  if (!cards.length || !chips.length) return {};

  let group = 'all';
  let streamingOnly = false;

  const apply = () => {
    let shown = 0;
    cards.forEach((card) => {
      const matches =
        (group === 'all' || (card.dataset.groups ?? '').split(' ').includes(group)) &&
        (!streamingOnly || card.hasAttribute('data-streaming'));
      card.hidden = !matches;
      if (matches) shown += 1;
    });
    chips.forEach((chip) => chip.setAttribute('aria-pressed', String(chip.dataset.filter === group)));
    streamingToggle?.setAttribute('aria-pressed', String(streamingOnly));
    text(count, format(strings.modelCount, { shown, total: cards.length }));
    if (empty) empty.hidden = shown > 0;
  };

  chips.forEach((chip) =>
    chip.addEventListener('click', () => { group = chip.dataset.filter ?? 'all'; apply(); }, { signal }),
  );
  streamingToggle?.addEventListener('click', () => { streamingOnly = !streamingOnly; apply(); }, { signal });

  return { destroy: () => cards.forEach((card) => { card.hidden = false; }) };
};

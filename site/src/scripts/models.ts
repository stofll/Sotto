import type { Behaviour, Runtime } from './runtime';

/** Model catalog: a radio group with a roving tab stop, plus the local/cloud tabs. */
export const initModels = ({ query, all, signal, text, strings }: Runtime): Behaviour => {
  const radios = all<HTMLElement>('[data-model]');

  const selectModel = (id: string, focus = false) => {
    const detail = strings.models[id];
    if (!detail) return;

    radios.forEach((radio) => {
      const checked = radio.dataset.model === id;
      radio.setAttribute('aria-checked', String(checked));
      radio.tabIndex = checked ? 0 : -1;
      radio.classList.toggle('is-selected', checked);
      if (checked && focus) radio.focus();
    });

    text(query('[data-model-detail]'), detail);
  };

  radios.forEach((radio, index) => {
    radio.addEventListener('click', () => selectModel(radio.dataset.model ?? ''), { signal });
    radio.addEventListener(
      'keydown',
      (event) => {
        let next = index;
        if (['ArrowDown', 'ArrowRight'].includes(event.key)) next = (index + 1) % radios.length;
        else if (['ArrowUp', 'ArrowLeft'].includes(event.key)) next = (index - 1 + radios.length) % radios.length;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = radios.length - 1;
        else return;

        event.preventDefault();
        selectModel(radios[next].dataset.model ?? '', true);
      },
      { signal },
    );
  });

  const tabs = all<HTMLElement>('[data-model-type]');

  const choosePanel = (name: string, focus = false) => {
    tabs.forEach((tab) => {
      const selected = tab.dataset.modelType === name;
      tab.classList.toggle('is-selected', selected);
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
      if (selected && focus) tab.focus();

      const panel = document.getElementById(tab.getAttribute('aria-controls') ?? '');
      if (panel) panel.hidden = !selected;
    });
  };

  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => choosePanel(tab.dataset.modelType ?? 'local'), { signal });
    tab.addEventListener(
      'keydown',
      (event) => {
        let next = index;
        if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') next = 1 - index;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = tabs.length - 1;
        else return;

        event.preventDefault();
        choosePanel(tabs[next].dataset.modelType ?? 'local', true);
      },
      { signal },
    );
  });

  return {};
};

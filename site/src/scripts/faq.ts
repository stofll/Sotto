import type { Behaviour, Runtime } from './runtime';

/** A cited answer should be readable immediately when its fragment is opened. */
export const initFaq = ({ signal }: Runtime): Behaviour => {
  const revealAnswer = () => {
    const id = location.hash.slice(1);
    if (!id.startsWith('faq-')) return;
    const answer = document.getElementById(id);
    if (answer instanceof HTMLDetailsElement) answer.open = true;
  };
  revealAnswer();
  window.addEventListener('hashchange', revealAnswer, { signal });
  return {};
};

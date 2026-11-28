import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defineComponent, h } from 'vue';
import { mount } from '@vue/test-utils';
import { useSecretReveal, type SecretReveal } from '../../src/composables/useSecretReveal';

function mountReveal(ttlMs?: number): { reveal: SecretReveal; unmount: () => void } {
  let api!: SecretReveal;

  const wrapper = mount(
    defineComponent({
      setup() {
        api = useSecretReveal(ttlMs);
        return () => h('div');
      }
    })
  );

  return { reveal: api, unmount: () => wrapper.unmount() };
}

beforeEach(() => {
  vi.useFakeTimers();
  // Reset the module's window-hidden flag between tests.
  window.dispatchEvent(new Event('focus'));
});

afterEach(() => {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => false
  });
  vi.useRealTimers();
});

describe('useSecretReveal (renderer:reveal-timeout)', () => {
  it('re-masks after the 10 s default and drops the value', async () => {
    const { reveal, unmount } = mountReveal();

    await reveal.reveal(async () => 'hunter2');
    expect(reveal.secret.value).toBe('hunter2');
    expect(reveal.isRevealed.value).toBe(true);

    vi.advanceTimersByTime(9_999);
    expect(reveal.isRevealed.value).toBe(true);

    vi.advanceTimersByTime(1);
    expect(reveal.secret.value).toBeNull();
    expect(reveal.isRevealed.value).toBe(false);

    unmount();
  });

  it('re-masks when the window loses focus', async () => {
    const { reveal, unmount } = mountReveal();
    await reveal.reveal(async () => 'hunter2');

    window.dispatchEvent(new Event('blur'));

    expect(reveal.secret.value).toBeNull();
    unmount();
  });

  it('re-masks when the document becomes hidden', async () => {
    const { reveal, unmount } = mountReveal();
    await reveal.reveal(async () => 'hunter2');

    Object.defineProperty(document, 'hidden', {
      configurable: true,
      get: () => true
    });
    document.dispatchEvent(new Event('visibilitychange'));

    expect(reveal.secret.value).toBeNull();
    unmount();
  });

  it('drops the value when the component unmounts', async () => {
    const { reveal, unmount } = mountReveal();
    await reveal.reveal(async () => 'hunter2');

    unmount();

    expect(reveal.secret.value).toBeNull();
  });

  it('does not reveal a value that resolves after hiding', async () => {
    const { reveal, unmount } = mountReveal();

    let resolve!: (value: string | null) => void;
    const pending = reveal.reveal(
      () =>
        new Promise<string | null>((done) => {
          resolve = done;
        })
    );

    reveal.hide();
    resolve('late');
    await pending;

    expect(reveal.secret.value).toBeNull();
    unmount();
  });

  it('does not fetch again while already revealed', async () => {
    const { reveal, unmount } = mountReveal();
    const fetch = vi.fn(async () => 'hunter2');

    await reveal.reveal(fetch);
    await reveal.reveal(fetch);

    expect(fetch).toHaveBeenCalledTimes(1);
    unmount();
  });

  it('toggles between revealed and hidden', async () => {
    const { reveal, unmount } = mountReveal();
    const fetch = vi.fn(async () => 'hunter2');

    await reveal.toggle(fetch);
    expect(reveal.isRevealed.value).toBe(true);

    await reveal.toggle(fetch);
    expect(reveal.isRevealed.value).toBe(false);
    expect(fetch).toHaveBeenCalledTimes(1);

    unmount();
  });
});

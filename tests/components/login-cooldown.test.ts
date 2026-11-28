import { afterEach, describe, expect, it, vi } from 'vitest';
import { defineComponent, h } from 'vue';
import { mount } from '@vue/test-utils';
import { formatCountdown, useLoginCooldown } from '../../src/composables/useLoginCooldown';

function mountCooldown() {
  let api!: ReturnType<typeof useLoginCooldown>;

  const wrapper = mount(
    defineComponent({
      setup() {
        api = useLoginCooldown();
        return () => h('div');
      }
    })
  );

  return { api, unmount: () => wrapper.unmount() };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('useLoginCooldown', () => {
  it('formats a second count as m:ss', () => {
    expect(formatCountdown(300)).toBe('5:00');
    expect(formatCountdown(59)).toBe('0:59');
    expect(formatCountdown(0)).toBe('0:00');
  });

  it('ticks down once per second and stops at zero', () => {
    vi.useFakeTimers();
    const { api, unmount } = mountCooldown();

    api.begin(3);
    expect(api.remaining.value).toBe(3);

    vi.advanceTimersByTime(1_000);
    expect(api.remaining.value).toBe(2);

    vi.advanceTimersByTime(1_000);
    expect(api.remaining.value).toBe(1);

    vi.advanceTimersByTime(1_000);
    expect(api.remaining.value).toBe(0);

    vi.advanceTimersByTime(5_000);
    expect(api.remaining.value).toBe(0);

    unmount();
  });

  it('restarts from a new value without double-ticking', () => {
    vi.useFakeTimers();
    const { api, unmount } = mountCooldown();

    api.begin(10);
    vi.advanceTimersByTime(2_000);
    api.begin(5);
    expect(api.remaining.value).toBe(5);

    vi.advanceTimersByTime(1_000);
    expect(api.remaining.value).toBe(4);

    unmount();
  });

  it('clears immediately', () => {
    vi.useFakeTimers();
    const { api, unmount } = mountCooldown();

    api.begin(10);
    api.clear();
    expect(api.remaining.value).toBe(0);

    vi.advanceTimersByTime(1_000);
    expect(api.remaining.value).toBe(0);

    unmount();
  });
});

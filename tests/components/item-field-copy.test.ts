import { afterEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { Check, Copy } from '@lucide/vue';
import ItemField from '../../src/components/ui/ItemField.vue';
import Button from '../../src/components/ui/Button.vue';

function mountField(copyAction: () => Promise<boolean>) {
  return mount(ItemField, {
    props: {
      label: 'password',
      displayValue: 'secret',
      canCopy: true,
      copyAction
    }
  });
}

afterEach(() => {
  vi.useRealTimers();
});

describe('ItemField copy feedback', () => {
  it('shows a tick after a successful copy and returns to the copy icon', async () => {
    vi.useFakeTimers();
    const wrapper = mountField(async () => true);
    const button = wrapper.findComponent(Button);

    expect(button.props('iconComponent')).toBe(Copy);

    await button.trigger('click');
    await flushPromises();
    expect(button.props('iconComponent')).toBe(Check);

    vi.advanceTimersByTime(2_000);
    await wrapper.vm.$nextTick();
    expect(button.props('iconComponent')).toBe(Copy);

    wrapper.unmount();
  });

  it('keeps the copy icon when the copy fails', async () => {
    const wrapper = mountField(async () => false);
    const button = wrapper.findComponent(Button);

    await button.trigger('click');
    await flushPromises();

    expect(button.props('iconComponent')).toBe(Copy);

    wrapper.unmount();
  });
});

// End-to-end proof that fiber mode renders live fiber/channel state.
// Drives the real backend (cargo run) over the WebSocket via the UI.

import { test, expect } from '@playwright/test';
import { waitForEditorReady, typeInEditor, clickRun } from './helpers';

// Top-level concurrent program: spawns a worker, hands a value over a channel
// via select. Yields exactly two fibers (top-level + worker) and one channel.
const CONCURRENT = `fn worker(ch: Channel<int>) {
    select {
        42 -> ch => {}
    }
}

let ch = chan(1)
spawn worker(ch)
select {
    <-ch => println("got")
}
`;

declare global {
  interface Window {
    __VM_STORE__: { getState: () => { vmState: unknown; output: string[] } };
  }
}

test.describe('Concurrent fiber inspection', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('fiber-mode run shows live fibers and channels', async ({ page }) => {
    // Enable fiber mode via the header toggle.
    const toggle = page.locator('[data-testid="fiber-mode-toggle"]');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-checked', 'true');

    await typeInEditor(page, CONCURRENT);
    await clickRun(page);

    // The VM store must receive a populated scheduler snapshot (>= 2 fibers,
    // >= 1 channel) — this only happens if the backend drove the scheduler and
    // streamed VmStateJson, and the frontend applied it.
    await page.waitForFunction(
      () => {
        const vm = window.__VM_STORE__?.getState?.().vmState as
          | { fibers: Record<string, unknown>; channels: Record<string, unknown> }
          | null;
        if (!vm) return false;
        return Object.keys(vm.fibers).length >= 2 && Object.keys(vm.channels).length >= 1;
      },
      { timeout: 25000 }
    );

    // The Fibers tab auto-selects and renders one entry per fiber.
    await expect(page.locator('[data-testid="tab-fibers"]')).toHaveAttribute('data-active', 'true');
    await expect(page.locator('.fiber-item')).toHaveCount(2);

    // And the program actually produced its output.
    await page.waitForFunction(
      () => (window.__VM_STORE__?.getState?.().output ?? []).join('').includes('got'),
      { timeout: 25000 }
    );
  });
});

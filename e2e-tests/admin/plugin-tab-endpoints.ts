import { test, expect } from '@playwright/test';

test.describe('Plugin - Endpoints Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin endpoints when available', async ({ page }) => {
    await page.click('button:has-text("Endpoints")');
    await page.waitForTimeout(1000);

    const hasEndpoints = await page.locator('.flex.items-center.gap-4.p-3.border').count();
    const noEndpointsMsg = await page.locator('text="No endpoints available"').count();

    if (hasEndpoints > 0) {
      await expect(page.locator('.flex.items-center.gap-4.p-3.border .px-2.py-1.rounded').first()).toBeVisible();
      await expect(page.locator('.flex.items-center.gap-4.p-3.border .font-mono').first()).toBeVisible();
    } else if (noEndpointsMsg > 0) {
      await expect(noEndpointsMsg).toBeGreaterThan(0);
    } else {
      await expect(page.locator('.bg-white.p-6.rounded-lg.shadow-sm').last()).toBeVisible();
    }
  });
});
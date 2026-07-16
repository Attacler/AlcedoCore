import { test, expect } from '@playwright/test';

test.describe('Plugin - Runtime Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display runtime information tab', async ({ page }) => {
    await page.click('button:has-text("Runtime")');
    await page.waitForTimeout(1000);

    const hasRuntimeInfo = await page.locator('text="Container ID"').count();
    const noRuntimeMsg = await page.locator('text="No runtime information available"').count();

    if (hasRuntimeInfo > 0) {
      await expect(page.locator('text="Container ID"')).toBeVisible();
    } else {
      await expect(noRuntimeMsg).toBeGreaterThan(0);
    }
  });
});

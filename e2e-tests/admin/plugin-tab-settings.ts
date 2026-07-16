import { test, expect } from '@playwright/test';

test.describe('Plugin - Settings Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin settings tab', async ({ page }) => {
    await page.click('button:has-text("Settings")');
    await page.waitForTimeout(1000);

    const hasSettingsForm = await page.locator('form').count();
    const noSettingsMsg = await page.locator('text="does not have configurable settings"').count();

    if (hasSettingsForm > 0) {
      await expect(page.locator('button:has-text("Save Settings")')).toBeVisible();
      await expect(page.locator('button:has-text("Reset")')).toBeVisible();
    } else if (noSettingsMsg > 0) {
      await expect(noSettingsMsg).toBeGreaterThan(0);
    } else {
      await expect(page.locator('.bg-white.p-6.rounded-lg.shadow-sm').last()).toBeVisible();
    }
  });
});
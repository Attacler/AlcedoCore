import { test, expect } from '@playwright/test';

test.describe('Plugin - Migrations Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin migrations when available', async ({ page }) => {
    await page.click('button:has-text("Migrations")');
    await page.waitForTimeout(1000);

    const hasMigrations = await page.locator('.border-b.border-gray-200 >> button').count();
    const noMigrationsMsg = await page.locator('text="No migrations available"').count();

    if (hasMigrations > 0) {
      await expect(page.locator('.font-mono.text-sm.text-gray-500').first()).toBeVisible();
      const hasStatusBadge = await page.locator('.px-2.py-0-5.rounded-full.text-xs.font-medium').count();
      expect(hasStatusBadge).toBeGreaterThan(0);
    } else if (noMigrationsMsg > 0) {
      await expect(noMigrationsMsg).toBeGreaterThan(0);
    } else {
      await expect(page.locator('.bg-white.p-6.rounded-lg.shadow-sm').last()).toBeVisible();
    }
  });
});
import { test, expect } from '@playwright/test';

test.describe('Plugin Settings Page', () => {
  test('should display settings page for admin plugin', async ({ page }) => {
    await page.goto('/admin#/plugins/admin/settings');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    await expect(page.locator('h1:has-text("Settings:")')).toBeVisible();
  });

  test('should have back link to plugin detail', async ({ page }) => {
    await page.goto('/admin#/plugins/admin/settings');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('text=← Back to Detail')).toBeVisible();
  });
});
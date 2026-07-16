import { test, expect } from '@playwright/test';

test.describe('Plugin Detail Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin detail page with tabs', async ({ page }) => {
    await expect(page.locator('button:has-text("Documentation")')).toBeVisible();
    await expect(page.locator('button:has-text("Pages")')).toBeVisible();
    await expect(page.locator('button:has-text("Endpoints")')).toBeVisible();
    await expect(page.locator('button:has-text("Schema")')).toBeVisible();
    await expect(page.locator('button:has-text("Migrations")')).toBeVisible();
    await expect(page.locator('button:has-text("Settings")')).toBeVisible();
    await expect(page.locator('button:has-text("Logs")')).toBeVisible();
    await expect(page.locator('button:has-text("Docker")')).toBeVisible();
  });

  test('should navigate through all tabs on plugin detail', async ({ page }) => {
    const tabs = ['Documentation', 'Pages', 'Endpoints', 'Schema', 'Migrations', 'Settings', 'Logs', 'Docker'];

    for (const tab of tabs) {
      await page.click(`button:has-text("${tab}")`);
      await expect(page.locator(`button:has-text("${tab}")`)).toBeVisible();
      await page.waitForTimeout(500);
    }
  });

  test('should have back link to plugins', async ({ page }) => {
    await expect(page.locator('text=← Back to Plugins')).toBeVisible();
  });
});
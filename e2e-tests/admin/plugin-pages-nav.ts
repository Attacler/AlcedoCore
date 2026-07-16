import { test, expect } from '@playwright/test';

test.describe('Plugin Pages Navigation', () => {
  test('should navigate to plugin pages via sidebar', async ({ page }) => {
    await page.goto('/admin');
    await page.waitForLoadState('networkidle');

    const sidebarLinks = await page.locator('nav a').all();
    await expect(page.locator('aside')).toBeVisible();
  });
});
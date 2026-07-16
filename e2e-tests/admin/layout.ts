const { test, expect } = require('@playwright/test');

test.describe('Layout', () => {
  test('should display sidebar navigation', async ({ page }) => {
    await page.goto('/admin');
    await page.waitForLoadState('networkidle');

    const sidebar = page.locator('aside');
    await expect(sidebar).toBeVisible();
    await expect(page.getByRole('link', { name: 'extension Plugins' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Registries' })).toBeVisible();
  });

  test('should display header with title', async ({ page }) => {
    await page.goto('/admin');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('text=Admin Plugin Dashboard')).toBeVisible();
  });
});
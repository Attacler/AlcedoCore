const { test, expect } = require('@playwright/test');

test.describe('Dashboard Page', () => {
  test('should display dashboard with metric cards', async ({ page }) => {
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');

    const cards = page.locator('[class*="bg-white rounded-lg"][class*="shadow-sm"]');
    await expect(cards).toHaveCount(5);

    await expect(page.getByText('Total Plugins', { exact: true })).toBeVisible();
    await expect(page.getByText('Enabled', { exact: true })).toBeVisible();
    await expect(page.getByText('Disabled', { exact: true })).toBeVisible();
    await expect(page.getByText('System', { exact: true })).toBeVisible();
    await expect(page.getByText('User', { exact: true })).toBeVisible();
  });

  test('should display status message', async ({ page }) => {
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('text=All systems operational')).toBeVisible();
  });
});
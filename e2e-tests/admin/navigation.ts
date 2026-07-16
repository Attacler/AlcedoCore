import { test, expect } from '@playwright/test';

test.describe('Navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin');
    await page.waitForLoadState('networkidle');
  });

  test('should navigate to Dashboard via sidebar', async ({ page }) => {
    await page.click('text=Dashboard');
    await expect(page).toHaveURL(/.*\/admin#\/dashboard/);
    await expect(page.locator('h1:has-text("Admin Plugin Dashboard")')).toBeVisible();
  });

  test('should navigate to Plugins page via sidebar', async ({ page }) => {
    await page.click('text=Plugins');
    await expect(page).toHaveURL(/.*\/admin#\/plugins/);
  });

  test('should navigate to Registries page via sidebar', async ({ page }) => {
    await page.click('text=Registries');
    await expect(page).toHaveURL(/.*\/admin#\/registries/);
  });

  test('should navigate to Settings via sidebar', async ({ page }) => {
    await page.click('text=Settings');
    await expect(page).toHaveURL(/.*\/admin#\/settings/);
  });
});
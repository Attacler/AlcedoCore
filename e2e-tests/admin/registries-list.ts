import { test, expect } from '@playwright/test';

test.describe('Registries List Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/registries');
    await page.waitForLoadState('networkidle');
  });

  test('should display registries page', async ({ page }) => {
    await expect(page.locator('h2:has-text("Registries")')).toBeVisible();
    await expect(page.locator('text=Add Registry')).toBeVisible();
  });

  test('should navigate to Add Registry', async ({ page }) => {
    await page.click('text=Add Registry');
    await expect(page).toHaveURL(/.*\/admin#\/registries\/new/);
    await expect(page.locator('h1:has-text("Add Registry")')).toBeVisible();
  });
});
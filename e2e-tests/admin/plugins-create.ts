import { test, expect } from '@playwright/test';

test.describe('Plugin Create Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/new');
    await page.waitForLoadState('networkidle');
  });

  test('should display create plugin form', async ({ page }) => {
    await expect(page.locator('h1:has-text("Create Plugin")')).toBeVisible();
    await expect(page.locator('input[placeholder="my-plugin"]')).toBeVisible();
    await expect(page.locator('input[placeholder="1.0.0"]')).toBeVisible();
    await expect(page.locator('textarea[placeholder="Plugin description..."]')).toBeVisible();
  });

  test('should have plugin type selector', async ({ page }) => {
    const select = page.locator('select');
    await expect(select).toBeVisible();
    await expect(select.locator('option')).toHaveCount(2);
  });

  test('should have file upload input', async ({ page }) => {
    await expect(page.locator('input[type="file"]')).toBeVisible();
  });

  test('should have back link to plugins', async ({ page }) => {
    const backLink = page.locator('text=← Back to Plugins');
    await expect(backLink).toBeVisible();
  });
});
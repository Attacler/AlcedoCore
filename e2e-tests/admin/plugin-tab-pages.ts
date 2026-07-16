const { test, expect } = require('@playwright/test');

test.describe('Plugin - Pages Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/hello-world');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin page cards', async ({ page }) => {
    await page.click('button:has-text("Pages")');
    await page.waitForTimeout(1000);

    await expect(page.locator('a:has-text("/index")')).toBeVisible();
    await expect(page.locator('a:has-text("/counter")')).toBeVisible();
  });

  test('should show page names in cards', async ({ page }) => {
    await page.click('button:has-text("Pages")');
    await page.waitForTimeout(1000);

    await expect(page.locator('h4:has-text("Hello")')).toBeVisible();
    await expect(page.locator('h4:has-text("Counter")')).toBeVisible();
  });

  test('should show page routes as subheadings', async ({ page }) => {
    await page.click('button:has-text("Pages")');
    await page.waitForTimeout(1000);

    await expect(page.locator('p:has-text("/index")')).toBeVisible();
    await expect(page.locator('p:has-text("/counter")')).toBeVisible();
  });
});
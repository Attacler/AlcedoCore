const { test, expect } = require('@playwright/test');

test.describe('Plugin - Documentation Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/hello-world');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display documentation content', async ({ page }) => {
    await page.click('button:has-text("Documentation")');
    await page.waitForTimeout(1000);

    await expect(page.locator('h1:has-text("Getting Started with Hello-World Plugin")')).toBeVisible();
    await expect(page.locator('th:has-text("Option")')).toBeVisible();
    await expect(page.locator('th:has-text("Description")')).toBeVisible();
    await expect(page.locator('th:has-text("Default")')).toBeVisible();
  });

  test('should display configuration table', async ({ page }) => {
    await page.click('button:has-text("Documentation")');
    await page.waitForTimeout(1000);

    const table = page.locator('table');
    await expect(table).toBeVisible();
  });

  test('should display documentation file tree', async ({ page }) => {
    await page.click('button:has-text("Documentation")');
    await page.waitForTimeout(1000);

    await expect(page.locator('button:has-text("📄 getting-started.md")')).toBeVisible();
    await expect(page.locator('button:has-text("📄 api-reference.md")')).toBeVisible();
    await expect(page.locator('button:has-text("📁 guides")')).toBeVisible();
  });
});
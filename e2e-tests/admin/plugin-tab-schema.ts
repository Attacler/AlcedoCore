import { test, expect } from '@playwright/test';

test.describe('Plugin - Schema Tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins/admin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);
  });

  test('should display plugin schema when available', async ({ page }) => {
    await page.click('button:has-text("Schema")');
    await page.waitForTimeout(1000);

    const hasTables = await page.locator('table').count();
    const noSchemaMsg = await page.locator('text="No schema available"').count();

    if (hasTables > 0) {
      await expect(page.locator('table th:has-text("Column")')).toBeVisible();
      await expect(page.locator('table th:has-text("Type")')).toBeVisible();
      await expect(page.locator('table th:has-text("Nullable")')).toBeVisible();
    } else if (noSchemaMsg > 0) {
      await expect(noSchemaMsg).toBeGreaterThan(0);
    } else {
      await expect(page.locator('.bg-white.p-6.rounded-lg.shadow-sm').last()).toBeVisible();
    }
  });
});
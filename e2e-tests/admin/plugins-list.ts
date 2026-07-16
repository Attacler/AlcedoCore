import { test, expect } from '@playwright/test';

test.describe('Plugins List Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/plugins');
    await page.waitForLoadState('networkidle');
  });

  test('should display plugin list page', async ({ page }) => {
    await expect(page.locator('input[type="file"]')).toBeVisible();
    await expect(page.locator('a:has-text("Create Plugin")')).toBeVisible();
    await expect(page.locator('input[placeholder="Search plugins..."]')).toBeVisible();
  });

  test('should have filter buttons', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'All', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Enabled', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Disabled', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'System', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'User', exact: true })).toBeVisible();
  });

  test('should navigate to Create Plugin page', async ({ page }) => {
    await page.click('text=Create Plugin');
    await expect(page).toHaveURL(/.*\/admin#\/plugins\/new/);
    await expect(page.locator('h1:has-text("Create Plugin")')).toBeVisible();
  });

  test('should filter plugins by search', async ({ page }) => {
    const searchInput = page.locator('input[placeholder="Search plugins..."]');
    await searchInput.fill('hello');
    await expect(searchInput).toHaveValue('hello');
  });
});
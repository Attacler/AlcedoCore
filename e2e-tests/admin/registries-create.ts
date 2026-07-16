import { test, expect } from '@playwright/test';

test.describe('Registry Create/Edit Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/registries/new');
    await page.waitForLoadState('networkidle');
  });

  test('should display Add Registry form', async ({ page }) => {
    await expect(page.locator('h1:has-text("Add Registry")')).toBeVisible();
    await expect(page.locator('input[placeholder="My Registry"]')).toBeVisible();
    await expect(page.locator('input[placeholder="https://registry.example.com"]')).toBeVisible();
  });

  test('should have authentication dropdown', async ({ page }) => {
    const authSelect = page.locator('select').first();
    await expect(authSelect).toBeVisible();
    await expect(authSelect.locator('option')).toHaveCount(3);
  });

  test('should show/hide credentials based on auth type', async ({ page }) => {
    const usernameInput = page.locator('input[placeholder="admin"]');
    const passwordInput = page.locator('input[type="password"]');

    await page.selectOption('select', 'basic');
    await expect(usernameInput).toBeVisible();
    await expect(passwordInput).toBeVisible();
  });

  test('should have Save and Cancel buttons', async ({ page }) => {
    await expect(page.locator('button:has-text("Save")')).toBeVisible();
    await expect(page.locator('text=Cancel')).toBeVisible();
  });

  test('should have back link', async ({ page }) => {
    await expect(page.locator('text=← Back to Registries')).toBeVisible();
  });
});
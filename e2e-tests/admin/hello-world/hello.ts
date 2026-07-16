import { test, expect } from '@playwright/test';

test.describe('Sample Plugin - Hello Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/p/hello-world/index');
    await page.waitForLoadState('networkidle');
  });

  test('should display Hello page with default greeting', async ({ page }) => {
    await expect(page.locator('h1:has-text("Hello World!")')).toBeVisible();
  });

  test('should have name input field', async ({ page }) => {
    const nameInput = page.locator('input[placeholder="Enter your name"]');
    await expect(nameInput).toBeVisible();
    await expect(nameInput).toHaveValue('World');
  });

  test('should update greeting when name is changed and Update is clicked', async ({ page }) => {
    const nameInput = page.locator('input[placeholder="Enter your name"]');
    await nameInput.clear();
    await nameInput.fill('Alcedo');
    await page.click('button:has-text("Update")');
    await page.waitForTimeout(500);
    await expect(page.locator('h1:has-text("Hello Alcedo!")')).toBeVisible();
  });

  test('should have Update button', async ({ page }) => {
    await expect(page.locator('button:has-text("Update")')).toBeVisible();
  });

  test('should have navigation link to Counter page', async ({ page }) => {
    await expect(page.locator('a:has-text("Counter")').first()).toBeVisible();
  });

  test('should navigate to Counter page via link', async ({ page }) => {
    await page.locator('a:has-text("Counter")').first().click();
    await page.waitForLoadState('networkidle');
    await expect(page).toHaveURL(/.*\/admin#\/p\/hello-world\/counter/);
    await expect(page.locator('h1:has-text("Counter:")')).toBeVisible();
  });
});
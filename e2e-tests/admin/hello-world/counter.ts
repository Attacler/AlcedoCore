const { test, expect } = require('@playwright/test');

test.describe('Sample Plugin - Counter Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/admin#/p/hello-world/counter');
    await page.waitForLoadState('networkidle');
  });

  test('should display Counter page with initial value of 0', async ({ page }) => {
    await expect(page.locator('h1:has-text("Counter: 0")')).toBeVisible();
  });

  test('should increment counter when + button is clicked', async ({ page }) => {
    await page.click('button:has-text("+")');
    await page.waitForTimeout(500);
    await expect(page.locator('h1:has-text("Counter: 1")')).toBeVisible();
  });

  test('should decrement counter when - button is clicked', async ({ page }) => {
    await page.click('button:has-text("+")');
    await page.waitForTimeout(500);
    await page.click('button:has-text("-")');
    await page.waitForTimeout(500);
    await expect(page.locator('h1:has-text("Counter: 0")')).toBeVisible();
  });

  test('should increment multiple times correctly', async ({ page }) => {
    for (let i = 0; i < 3; i++) {
      await page.click('button:has-text("+")');
      await page.waitForTimeout(300);
    }
    await expect(page.locator('h1:has-text("Counter: 3")')).toBeVisible();
  });

  test('should have navigation link to Hello page', async ({ page }) => {
    await expect(page.locator('a:has-text("Hello")').first()).toBeVisible();
  });

  test('should navigate to Hello page via link', async ({ page }) => {
    await page.goto('/admin#/p/hello-world/index');
    await page.waitForLoadState('networkidle');
    await expect(page.locator('h1:has-text("Hello")')).toBeVisible();
  });
});
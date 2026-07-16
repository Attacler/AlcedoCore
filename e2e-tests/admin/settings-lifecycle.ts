import { test, expect } from '@playwright/test';

test.describe('Settings Lifecycle', () => {
  test.beforeEach(async ({ page }) => {
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          site_name: 'Test Site',
          logo_url: '',
          favicon_url: '',
          default_view_mode: 'table',
          default_page_size: 25,
        }
      }
    });
    await page.goto('/admin#/settings');
    await page.waitForLoadState('networkidle');
  });

  test('should display all 4 category cards on the index page', async ({ page }) => {
    await expect(page.locator('a:has(div:has-text("General"))')).toBeVisible();
    await expect(page.locator('a:has(div:has-text("Menu"))')).toBeVisible();
    await expect(page.locator('a:has(div:has-text("Branding"))')).toBeVisible();
    await expect(page.locator('a:has(div:has-text("Preferences"))')).toBeVisible();
  });

  test('should navigate to category page, edit default_page_size, save, reload, verify persistence', async ({ page }) => {
    // Navigate to Preferences category
    await page.locator('a:has(div:has-text("Preferences"))').click();
    await page.waitForLoadState('networkidle');

    const pageSizeRow = page.locator('div:has(div:has-text("Default Page Size"))');
    await expect(pageSizeRow).toBeVisible();

    const numberInput = pageSizeRow.locator('input[type="number"]');
    await numberInput.fill('50');

    const saveButton = pageSizeRow.locator('button:has-text("Save")');
    await saveButton.click();
    await page.waitForTimeout(1000);

    await page.reload();
    await page.waitForLoadState('networkidle');

    // Navigate back to Preferences
    await page.locator('a:has(div:has-text("Preferences"))').click();
    await page.waitForLoadState('networkidle');

    const reloadedInput = page.locator('div:has(div:has-text("Default Page Size"))').locator('input[type="number"]');
    await expect(reloadedInput).toHaveValue('50');
  });

  test('should cancel unsaved changes and revert to saved value', async ({ page }) => {
    await page.locator('a:has(div:has-text("Preferences"))').click();
    await page.waitForLoadState('networkidle');

    const input = page.locator('div:has(div:has-text("Default Page Size"))').locator('input[type="number"]');
    const initialValue = await input.inputValue();

    const newValue = initialValue === '25' ? '50' : '25';
    await input.fill(newValue);
    await page.waitForTimeout(200);

    await page.locator('button:has-text("Cancel")').click();
    await page.waitForTimeout(500);

    await expect(input).toHaveValue(initialValue);
  });

  test('should search and filter settings within a category page', async ({ page }) => {
    await page.locator('a:has(div:has-text("Preferences"))').click();
    await page.waitForLoadState('networkidle');

    const searchInput = page.locator('input[placeholder="Search settings..."]');
    await expect(searchInput).toBeVisible();

    await searchInput.fill('page');
    await page.waitForTimeout(300);

    await expect(searchInput).toHaveValue('page');

    await expect(page.locator('div:has(div:has-text("Default Page Size"))').first()).toBeVisible();

    await searchInput.fill('');
    await page.waitForTimeout(300);
  });

  test('should edit a string setting on its category page and save correctly', async ({ page }) => {
    await page.locator('a:has(div:has-text("Branding"))').click();
    await page.waitForLoadState('networkidle');

    const siteNameRow = page.locator('div:has(div:has-text("Site Name"))');
    await expect(siteNameRow).toBeVisible();

    const nameInput = siteNameRow.locator('input[type="text"]');
    await nameInput.fill('My Custom Site');

    await siteNameRow.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    await page.reload();
    await page.waitForLoadState('networkidle');

    await page.locator('a:has(div:has-text("Branding"))').click();
    await page.waitForLoadState('networkidle');

    const reloadedInput = page.locator('div:has(div:has-text("Site Name"))').locator('input[type="text"]');
    await expect(reloadedInput).toHaveValue('My Custom Site');
  });
});

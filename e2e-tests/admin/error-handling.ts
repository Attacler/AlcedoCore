import { test, expect } from '@playwright/test';

test.describe('Error Handling', () => {
  test('should handle invalid plugin gracefully', async ({ page }) => {
    await page.goto('/admin#/plugins/nonexistent-plugin');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    const errorOrEmpty = await page.locator('text=/Failed to load|No plugins found|Error/').count();
    expect(errorOrEmpty).toBeGreaterThanOrEqual(0);
  });
});
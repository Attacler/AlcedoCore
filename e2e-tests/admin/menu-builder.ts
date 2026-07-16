import { test, expect } from '@playwright/test';

test.describe('Menu Builder', () => {
  test.beforeEach(async ({ page }) => {
    // Seed clean state: clear menu_sections via API
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          menu_sections: [],
        }
      }
    });
    await page.goto('/admin#/menu-builder');
    await page.waitForLoadState('networkidle');
  });

  test('should show empty state when no sections exist', async ({ page }) => {
    // After clearing menu via API, the builder should show empty state
    await expect(page.locator('h3:has-text("No sections yet")')).toBeVisible();
    await expect(page.locator('button:has-text("+ Add Section")')).toBeVisible();
  });

  test('should create a section and verify it appears in the tree', async ({ page }) => {
    await page.waitForTimeout(500);

    // Click "+ Add Section" button (in empty state) or "Section" button (in toolbar)
    const addSectionBtn = page.locator('button:has-text("+ Add Section")');
    const sectionBtn = page.locator('button:has-text("Section")');
    if (await addSectionBtn.isVisible()) {
      await addSectionBtn.click();
    } else {
      await sectionBtn.click();
    }
    await page.waitForTimeout(300);

    // The new section should appear with a default label "New Section"
    await expect(page.locator('text=New Section').first()).toBeVisible();

    // Double-click the section label to start inline editing
    const sectionLabel = page.locator('span:has-text("New Section")').first();
    await sectionLabel.dblclick();
    await page.waitForTimeout(200);

    // The editing input should appear (v-focus directive auto-focuses it)
    const editInput = page.locator('input:focus');
    await editInput.fill('Test Section');
    await editInput.press('Enter');
    await page.waitForTimeout(200);

    // Verify the section label updated
    await expect(page.locator('span:has-text("Test Section")').first()).toBeVisible();
  });

  test('should add items to a section and edit item properties', async ({ page }) => {
    // Create a section first
    const addSectionBtn = page.locator('button:has-text("+ Add Section")');
    const sectionBtn = page.locator('button:has-text("Section")');
    if (await addSectionBtn.isVisible()) {
      await addSectionBtn.click();
    } else {
      await sectionBtn.click();
    }
    await page.waitForTimeout(300);

    // Rename section
    const sectionLabel = page.locator('span:has-text("New Section")').first();
    await sectionLabel.dblclick();
    await page.waitForTimeout(200);
    const editInput = page.locator('input:focus');
    await editInput.fill('Test Section');
    await editInput.press('Enter');
    await page.waitForTimeout(200);

    // Click the add item button (the + icon button in the section header)
    const addItemBtn = page.locator('button[title="Add item"]').first();
    await addItemBtn.click();
    await page.waitForTimeout(300);

    // The new item "New Item" should appear in the section
    await expect(page.locator('span:has-text("New Item")').first()).toBeVisible();

    // Click the item to select it and open the properties panel
    await page.locator('span:has-text("New Item")').first().click();
    await page.waitForTimeout(200);

    // Right panel should show "Item Properties"
    await expect(page.locator('h3:has-text("Item Properties")')).toBeVisible();

    // Edit the item label in the properties panel
    const labelInput = page.locator('input[placeholder="Menu item label"]');
    await expect(labelInput).toBeVisible();
    await labelInput.fill('Dashboard Link');

    // Set the route path
    await page.locator('input[placeholder="/collections"]').fill('/dashboard');
    await page.waitForTimeout(200);

    // Verify the item label updated in the tree
    await expect(page.locator('span:has-text("Dashboard Link")').first()).toBeVisible();
  });

  test('should hide an item via visibility toggle and verify it persists after save', async ({ page }) => {
    // Seed a menu section with items via API for deterministic state
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          menu_sections: [
            {
              id: 'test-section-1',
              label: 'Test Section',
              icon: 'folder',
              visible: true,
              items: [
                { id: 'item-1', label: 'Visible Item', icon: 'link', visible: true, route: '/dashboard' },
                { id: 'item-2', label: 'Hidden Item', icon: 'link', visible: true, route: '/plugins' },
              ]
            }
          ]
        }
      }
    });

    // Reload the menu builder
    await page.reload();
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    // Verify both items are visible
    await expect(page.locator('span:has-text("Visible Item")').first()).toBeVisible();
    await expect(page.locator('span:has-text("Hidden Item")').first()).toBeVisible();

    // Click the "Hidden Item" to select it
    await page.locator('span:has-text("Hidden Item")').first().click();
    await page.waitForTimeout(200);

    // Find the visibility toggle in the properties panel
    // It's inside a div.flex.items-center.justify-between with "Visible" text
    const visibilitySection = page.locator('div.flex.items-center.justify-between:has(div:has-text("Visible"))');
    const toggleButton = visibilitySection.locator('button');
    await toggleButton.click();
    await page.waitForTimeout(200);

    // Save the menu
    await page.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Navigate to Dashboard to verify sidebar
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(1000);

    // The "Test Section" should be visible in the sidebar
    await expect(page.locator('aside div:has-text("Test Section")').first()).toBeVisible();

    // "Visible Item" should be visible in sidebar as a router-link
    await expect(page.locator('aside a:has-text("Visible Item")').first()).toBeVisible();

    // "Hidden Item" should NOT be visible in the sidebar
    await expect(page.locator('aside a:has-text("Hidden Item")')).toHaveCount(0);

    // Navigate back to menu builder — hidden item should still be in the editor
    await page.goto('/admin#/menu-builder');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);
    await expect(page.locator('span:has-text("Hidden Item")').first()).toBeVisible();
  });

  test('should delete a section and verify it disappears from sidebar after save', async ({ page }) => {
    // Seed menu via API
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          menu_sections: [
            {
              id: 'del-section-1',
              label: 'Delete Me',
              icon: 'folder',
              visible: true,
              items: [{ id: 'del-item-1', label: 'Delete Item', icon: 'link', visible: true, route: '/settings' }]
            }
          ]
        }
      }
    });

    await page.reload();
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    // Find the delete button on the section header
    const deleteSectionBtn = page.locator('button[title="Delete section"]').first();
    await deleteSectionBtn.click();
    await page.waitForTimeout(300);

    // Confirmation dialog should appear
    await expect(page.locator('h3:has-text("Delete Section")')).toBeVisible();
    await page.locator('button:has-text("Delete")').click();
    await page.waitForTimeout(200);

    // Save
    await page.locator('button:has-text("Save")').click();
    await page.waitForTimeout(1000);

    // Navigate to dashboard
    await page.goto('/admin#/dashboard');
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    // The deleted section should not be in the sidebar
    await expect(page.locator('aside div:has-text("Delete Me")')).toHaveCount(0);
  });

  test('should support external URL items', async ({ page }) => {
    // Seed a section with external item
    await page.request.post('/api/settings/batch', {
      data: {
        settings: {
          menu_sections: [
            {
              id: 'ext-section',
              label: 'External Test',
              icon: 'folder',
              visible: true,
              items: [
                { id: 'ext-item', label: 'Docs', icon: 'link', visible: true, route: '', url: 'https://docs.example.com', external: true }
              ]
            }
          ]
        }
      }
    });

    await page.reload();
    await page.waitForLoadState('networkidle');
    await page.waitForTimeout(500);

    // Select the item and verify it shows external URL input
    await page.locator('span:has-text("Docs")').first().click();
    await page.waitForTimeout(200);
    await expect(page.locator('h3:has-text("Item Properties")')).toBeVisible();

    // Verify external URL input shows the configured value
    const externalUrlInput = page.locator('input[placeholder="https://docs.example.com"]');
    await expect(externalUrlInput).toHaveValue('https://docs.example.com');
  });
});

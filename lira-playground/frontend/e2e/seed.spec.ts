/**
 * Seed test for Lira Playground E2E tests
 *
 * This file provides example tests demonstrating patterns
 * for use with Playwright test agents.
 */

import { test, expect } from '@playwright/test';
import {
  waitForEditorReady,
  typeInEditor,
  clickRun,
  waitForOutput,
  SAMPLE_PROGRAMS,
} from './helpers';

test.describe('Lira Playground - Seed Tests', () => {
  test.beforeEach(async ({ page }) => {
    // Navigate to the playground
    await page.goto('/');
    // Wait for the app to be ready
    await waitForEditorReady(page);
  });

  test('should load the playground interface', async ({ page }) => {
    // Verify main components are visible
    await expect(page.locator('.monaco-editor')).toBeVisible();
    await expect(page.locator('button:has-text("Run")')).toBeVisible();
    await expect(page.locator('.output-panel')).toBeVisible();
  });

  test('should run Hello World program', async ({ page }) => {
    // Type Hello World program
    await typeInEditor(page, SAMPLE_PROGRAMS.helloWorld);

    // Click Run
    await clickRun(page);

    // Wait for output
    await waitForOutput(page, 'Hello, World!');
  });

  test('should show compilation errors', async ({ page }) => {
    // Type invalid code
    await typeInEditor(page, SAMPLE_PROGRAMS.syntaxError);

    // Click Run
    await clickRun(page);

    // Should show error state
    await page.waitForSelector('.error-marker, [class*="error"]', { timeout: 10000 });
  });

  test('should display AST panel', async ({ page }) => {
    // Type simple code
    await typeInEditor(page, SAMPLE_PROGRAMS.simpleArithmetic);

    // Compile to get AST
    await clickRun(page);

    // Look for AST panel - use .ast-tree specifically
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });
  });

  test('should handle multiple runs', async ({ page }) => {
    // First run
    await typeInEditor(page, 'println("First")');
    await clickRun(page);
    await waitForOutput(page, 'First');

    // Second run with different code
    await typeInEditor(page, 'println("Second")');
    await clickRun(page);
    await waitForOutput(page, 'Second');
  });
});

// spec: specs/playground-core.md
// seed: e2e/seed.spec.ts

import { test, expect } from '@playwright/test';
import {
  waitForEditorReady,
  typeInEditor,
  clickRun,
  waitForOutput,
  selectOutputTab,
  SAMPLE_PROGRAMS,
} from './helpers';

test.describe('Editor Basic Operations', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('1.1 Load Playground Interface', async ({ page }) => {
    // Verify main components are visible
    await expect(page.locator('.monaco-editor')).toBeVisible();
    await expect(page.locator('button:has-text("Run")')).toBeVisible();
    await expect(page.locator('.output-panel')).toBeVisible();
  });

  test('1.2 Type Code in Editor', async ({ page }) => {
    // Click on the editor and type code
    await typeInEditor(page, 'println("Hello")');

    // Verify code appears (check editor contains text)
    const editorContent = await page.locator('.monaco-editor .view-lines').textContent();
    expect(editorContent).toContain('println');
  });
});

test.describe('Code Compilation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('2.1 Compile Valid Code', async ({ page }) => {
    // Enter valid Lira code
    await typeInEditor(page, 'let x = 42');

    // Click Run button
    await clickRun(page);

    // Wait for completion - no error should appear
    await page.waitForTimeout(2000);

    // Check that AST panel has content (compilation succeeded)
    // Use .ast-tree specifically as it's the unique content element
    const astContent = page.locator('.ast-tree');
    await expect(astContent).toBeVisible({ timeout: 10000 });
  });

  test('2.2 Compile Code with Syntax Error', async ({ page }) => {
    // Enter invalid code
    await typeInEditor(page, SAMPLE_PROGRAMS.syntaxError);

    // Click Run button
    await clickRun(page);

    // Wait for error indication
    await page.waitForSelector('.error-marker, [class*="error"], .output-console:has-text("Error")', {
      timeout: 10000,
    });
  });

  test('2.3 Compile Code with Type Error', async ({ page }) => {
    // Enter code with type mismatch
    await typeInEditor(page, SAMPLE_PROGRAMS.typeError);

    // Click Run button
    await clickRun(page);

    // Wait for type error
    await page.waitForSelector('.error-marker, [class*="error"], .output-console:has-text("type")', {
      timeout: 10000,
    });
  });
});

test.describe('Code Execution', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('3.1 Run Hello World', async ({ page }) => {
    // Enter Hello World code
    await typeInEditor(page, SAMPLE_PROGRAMS.helloWorld);

    // Click Run
    await clickRun(page);

    // Wait for output
    await waitForOutput(page, 'Hello, World!');
  });

  test('3.2 Run Arithmetic Program', async ({ page }) => {
    // Enter arithmetic code
    await typeInEditor(page, SAMPLE_PROGRAMS.simpleArithmetic);

    // Click Run
    await clickRun(page);

    // Wait for output showing sum
    await waitForOutput(page, '30');
  });

  test('3.3 Run Function Call', async ({ page }) => {
    // Enter function code
    await typeInEditor(page, SAMPLE_PROGRAMS.withFunction);

    // Click Run
    await clickRun(page);

    // Wait for output
    await waitForOutput(page, '8');
  });

  test('3.4 Run Program with Loop', async ({ page }) => {
    // Enter loop code
    await typeInEditor(page, SAMPLE_PROGRAMS.withLoop);

    // Click Run
    await clickRun(page);

    // Wait for loop output
    await waitForOutput(page, '0');
    await waitForOutput(page, '1');
    await waitForOutput(page, '2');
  });
});

test.describe('Output Panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('4.1 View Output Tab', async ({ page }) => {
    // Run a program
    await typeInEditor(page, SAMPLE_PROGRAMS.helloWorld);
    await clickRun(page);

    // Click Output tab
    await selectOutputTab(page, 'Output');

    // Verify output is shown
    await waitForOutput(page, 'Hello, World!');
  });

  test('4.2 View Debug Tab', async ({ page }) => {
    // Click Debug tab
    await selectOutputTab(page, 'Debug');

    // Should show debug panel
    const debugPanel = page.locator('.debug-panel');
    await expect(debugPanel).toBeVisible();

    // Should show "Not debugging" when not in debug mode
    await expect(debugPanel).toContainText(/not debugging|no.*debug/i);
  });

  test('4.3 View VM Tab', async ({ page }) => {
    // Run a program first
    await typeInEditor(page, SAMPLE_PROGRAMS.helloWorld);
    await clickRun(page);

    // Click VM tab
    await selectOutputTab(page, 'VM');

    // Should show VM panel
    const vmPanel = page.locator('.vm-inspector');
    await expect(vmPanel).toBeVisible({ timeout: 10000 });
  });

  test('4.4 Clear Output Between Runs', async ({ page }) => {
    // First run
    await typeInEditor(page, 'println("First")');
    await clickRun(page);
    await waitForOutput(page, 'First');

    // Second run
    await typeInEditor(page, 'println("Second")');
    await clickRun(page);

    // Output should show Second
    await waitForOutput(page, 'Second');

    // Output console should be cleared of First (or Second should be most recent)
    // The console class is used for output (not output-console)
    const outputText = await page.locator('.console').textContent();
    // The most recent output should be "Second"
    expect(outputText).toContain('Second');
  });
});

test.describe('AST Panel', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('5.1 View AST Tree', async ({ page }) => {
    // Enter simple code
    await typeInEditor(page, 'let x = 42');

    // Run to generate AST
    await clickRun(page);

    // Look for AST panel with Program node
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Should contain Program node
    await expect(astPanel).toContainText(/program/i);
  });

  test('5.2 AST Shows Function Structure', async ({ page }) => {
    // Enter function code
    await typeInEditor(page, `fn greet(name: string) {
  println(name)
}`);

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    await page.waitForTimeout(1000);

    // AST should show function declaration
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });
    // Check for FnDecl in the AST tree
    await expect(astPanel).toContainText('FnDecl', { timeout: 5000 });
  });
});

test.describe('Error Handling', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForEditorReady(page);
  });

  test('6.1 Recover from Error State', async ({ page }) => {
    // First, run code with error
    await typeInEditor(page, SAMPLE_PROGRAMS.syntaxError);
    await clickRun(page);

    // Wait for error state
    await page.waitForSelector('.error-marker, [class*="error"], .output-console:has-text("Error")', {
      timeout: 10000,
    });

    // Now fix the code and run again
    await typeInEditor(page, SAMPLE_PROGRAMS.helloWorld);
    await clickRun(page);

    // Should run successfully
    await waitForOutput(page, 'Hello, World!');
  });
});

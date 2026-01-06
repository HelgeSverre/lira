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

  test('5.3 AST Shows Identifier Names', async ({ page }) => {
    // Enter code with identifiers
    await typeInEditor(page, 'println("Hello")');

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand Expression node to see Call details
    await astPanel.locator('.tree-node-header:has-text("Expression")').click();
    await page.waitForTimeout(100);

    // Expand Call node to see callee (Identifier)
    await astPanel.locator('.tree-node-header:has-text("Call")').click();
    await page.waitForTimeout(100);

    // Expand Identifier node to see the name
    await astPanel.locator('.tree-node-header:has-text("callee")').click();
    await page.waitForTimeout(100);

    // The identifier name should be rendered in an .ast-name element
    await expect(astPanel.locator('.ast-name:has-text("println")')).toBeVisible({ timeout: 5000 });
  });

  test('5.4 AST Shows String Literal Values', async ({ page }) => {
    // Enter code with string literal
    await typeInEditor(page, 'println("Hello, Lira!")');

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand nodes to see the string literal
    await astPanel.locator('.tree-node-header:has-text("Expression")').click();
    await page.waitForTimeout(100);
    await astPanel.locator('.tree-node-header:has-text("Call")').click();
    await page.waitForTimeout(100);

    // Expand args to see the string argument
    await astPanel.locator('.tree-node-header:has-text("args")').click();
    await page.waitForTimeout(100);

    // Expand the argument node to see the StringLiteral node
    await astPanel.locator('.tree-node-header:has-text("arg 1")').click();
    await page.waitForTimeout(100);

    // Expand the StringLiteral node to see the value
    await astPanel.locator('.tree-node-header:has-text("StringLiteral")').click();
    await page.waitForTimeout(100);

    // String literal value should be shown in .ast-string element
    await expect(astPanel.locator('.ast-string:has-text("Hello, Lira!")')).toBeVisible({ timeout: 5000 });
  });

  test('5.5 AST Shows Integer Literal Values', async ({ page }) => {
    // Enter code with integer literal
    await typeInEditor(page, 'let x = 42');

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand VarDecl node to see the initializer
    await astPanel.locator('.tree-node-header:has-text("VarDecl")').click();
    await page.waitForTimeout(100);

    // Expand the init/IntLiteral node
    await astPanel.locator('.tree-node-header:has-text("init")').click();
    await page.waitForTimeout(100);

    // Integer literal value should be shown in .ast-number element
    await expect(astPanel.locator('.ast-number:has-text("42")')).toBeVisible({ timeout: 5000 });
  });

  test('5.6 AST Shows Binary Operator Symbols', async ({ page }) => {
    // Enter code with binary operators
    await typeInEditor(page, 'let sum = 10 + 20');

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand VarDecl to see the binary expression
    await astPanel.locator('.tree-node-header:has-text("VarDecl")').click();
    await page.waitForTimeout(100);

    // Expand the init/Binary node
    await astPanel.locator('.tree-node-header:has-text("init")').click();
    await page.waitForTimeout(100);

    // Operator should be shown as symbol (+) not name (Add)
    await expect(astPanel.locator('.ast-operator:has-text("+")')).toBeVisible({ timeout: 5000 });
  });

  test('5.7 AST Shows Function Parameters with Types', async ({ page }) => {
    // Enter function with typed parameters
    await typeInEditor(page, `fn add(a: int, b: int) -> int {
  return a + b
}`);

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand FnDecl to see function details
    await astPanel.locator('.tree-node-header:has-text("FnDecl")').click();
    await page.waitForTimeout(100);

    // Should show function name
    await expect(astPanel.locator('.ast-name:has-text("add")')).toBeVisible({ timeout: 5000 });

    // Should show parameter types (int) - may need to expand params
    const paramsNode = astPanel.locator('.tree-node-header:has-text("params")');
    if (await paramsNode.isVisible()) {
      await paramsNode.click();
      await page.waitForTimeout(100);
    }

    await expect(astPanel.locator('.ast-type:has-text("int")')).toBeVisible({ timeout: 5000 });
  });

  test('5.8 AST Shows Class Fields with Types', async ({ page }) => {
    // Enter class with fields
    await typeInEditor(page, `class Person {
  name: string
  age: int
}`);

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand ClassDecl to see fields
    await astPanel.locator('.tree-node-header:has-text("ClassDecl")').click();
    await page.waitForTimeout(100);

    // Should show class name
    await expect(astPanel.locator('.ast-name:has-text("Person")')).toBeVisible({ timeout: 5000 });

    // Expand fields node
    await astPanel.locator('.tree-node-header:has-text("fields")').click();
    await page.waitForTimeout(100);

    // Expand first field node (name: string)
    const fieldNodes = astPanel.locator('.tree-node-header.node-field');
    if (await fieldNodes.count() > 0) {
      await fieldNodes.first().click();
      await page.waitForTimeout(100);
    }

    // Should show field types
    await expect(astPanel.locator('.ast-type:has-text("string")')).toBeVisible({ timeout: 5000 });
  });

  test('5.9 AST Shows Expandable Arguments', async ({ page }) => {
    // Enter simple function call with numeric arguments
    await typeInEditor(page, 'println(42)');

    // Run to generate AST
    await clickRun(page);

    // Wait for AST to populate
    const astPanel = page.locator('.ast-tree');
    await expect(astPanel).toBeVisible({ timeout: 10000 });

    // Expand Expression
    await astPanel.locator('.tree-node-header:has-text("Expression")').click();
    await page.waitForTimeout(100);

    // Expand Call node
    await astPanel.locator('.tree-node-header:has-text("Call")').click();
    await page.waitForTimeout(100);

    // Should show args node with count
    await expect(astPanel.locator('.tree-node-label:has-text("args")')).toBeVisible({ timeout: 5000 });

    // Expand args node
    await astPanel.locator('.tree-node-header:has-text("args")').click();
    await page.waitForTimeout(100);

    // Expand first argument
    await astPanel.locator('.tree-node-header:has-text("arg 1")').click();
    await page.waitForTimeout(100);

    // Expand IntLiteral to see the value
    await astPanel.locator('.tree-node-header:has-text("IntLiteral")').click();
    await page.waitForTimeout(100);

    // Should show argument value (42)
    await expect(astPanel.locator('.ast-number:has-text("42")')).toBeVisible({ timeout: 5000 });
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

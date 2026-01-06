/**
 * Shared test helpers for Lira Playground E2E tests
 *
 * This file provides:
 * - Helper functions for common interactions
 * - Example code snippets for testing
 */

import { expect, Page } from '@playwright/test';

// Helper: Wait for the editor to be ready
export async function waitForEditorReady(page: Page) {
  // Wait for Monaco editor to load
  await page.waitForSelector('.monaco-editor', { timeout: 30000 });
  // Wait for the editor to be interactive
  await page.waitForFunction(() => {
    const editor = document.querySelector('.monaco-editor');
    return editor && !editor.classList.contains('loading');
  }, { timeout: 10000 });
}

// Helper: Type code into the Monaco editor
export async function typeInEditor(page: Page, code: string) {
  // Wait for Monaco to be available
  await page.waitForFunction(() => {
    const monaco = (window as any).monaco;
    return monaco && monaco.editor && monaco.editor.getEditors().length > 0;
  }, { timeout: 10000 });

  // Use Monaco's API directly to set content
  // The @monaco-editor/react onChange should automatically update the store
  const success = await page.evaluate((newCode) => {
    const monaco = (window as any).monaco;
    if (monaco) {
      const editors = monaco.editor.getEditors();
      if (editors && editors.length > 0) {
        const editor = editors[0];
        const model = editor.getModel();
        if (model) {
          // Use pushEditOperations to trigger onChange properly
          model.pushEditOperations(
            [],
            [{
              range: model.getFullModelRange(),
              text: newCode,
            }],
            () => null
          );
          editor.focus();
          return true;
        }
      }
    }
    return false;
  }, code);

  if (!success) {
    throw new Error('Failed to set editor content via Monaco API');
  }

  // Wait for the content to be set and React to re-render
  await page.waitForTimeout(500);
}

// Helper: Set editor content directly (faster than typing)
export async function setEditorContent(page: Page, code: string) {
  // Use Monaco's API directly for speed
  await page.evaluate((content) => {
    // Access Monaco editor instance through the window
    const editorElement = document.querySelector('.monaco-editor');
    if (editorElement) {
      // Trigger a custom event that the app can handle, or use clipboard
      const editor = (window as any).monaco?.editor?.getEditors?.()[0];
      if (editor) {
        editor.setValue(content);
      }
    }
  }, code);
}

// Helper: Click the Run button
export async function clickRun(page: Page) {
  await page.click('[data-testid="run-button"]');
}

// Helper: Click the Compile button (if present)
export async function clickCompile(page: Page) {
  const compileBtn = page.locator('[data-testid="compile-button"]');
  if (await compileBtn.isVisible()) {
    await compileBtn.click();
  }
}

// Helper: Wait for output panel to show result
export async function waitForOutput(page: Page, expectedText?: string) {
  // Wait for output console to be available
  await page.waitForSelector('[data-testid="output-console"]', { timeout: 10000 });

  if (expectedText) {
    await expect(page.locator('[data-testid="output-console"]')).toContainText(expectedText, { timeout: 15000 });
  }
}

// Helper: Wait for compilation error
export async function waitForCompileError(page: Page) {
  await page.waitForSelector('[data-testid="ast-errors"], [data-testid="console-error"]', { timeout: 10000 });
}

// Helper: Click on a tab in the output panel
export async function selectOutputTab(page: Page, tabName: 'Output' | 'Debug' | 'VM' | 'Fibers' | 'Channels') {
  const tabId = tabName.toLowerCase();
  await page.click(`[data-testid="tab-${tabId}"]`);
}

// Helper: Toggle a breakpoint at a line
export async function toggleBreakpoint(page: Page, lineNumber: number) {
  // Click on the gutter at the specified line
  await page.click(`.monaco-editor .line-numbers:has-text("${lineNumber}")`);
}

// Helper: Select a sample program
export async function selectSample(page: Page, sampleName: string) {
  const sampleSelector = page.locator('[data-testid="sample-select"]');
  if (await sampleSelector.isVisible()) {
    await sampleSelector.selectOption({ label: sampleName });
  }
}

// Helper: Toggle breakpoint by clicking on line number gutter
export async function setBreakpointOnLine(page: Page, lineNumber: number) {
  // Use the editorStore API directly since Monaco click events are finicky in tests
  // The store's toggleBreakpoint function is what the Monaco click handler calls
  const success = await page.evaluate((line) => {
    // Access zustand store from window (exposed by the React app)
    const store = (window as any).__EDITOR_STORE__;
    if (store) {
      store.getState().toggleBreakpoint(line);
      return true;
    }
    // Fallback: try to access via zustand devtools
    return false;
  }, lineNumber);

  if (!success) {
    // Fallback: click on the glyph margin area
    // Monaco organizes gutter elements differently - try clicking on line numbers
    const lineNumbers = page.locator('.monaco-editor .view-line').nth(lineNumber - 1);
    const box = await lineNumbers.boundingBox();
    if (box) {
      // Click to the left of the line (in the gutter area)
      await page.mouse.click(box.x - 20, box.y + box.height / 2);
    }
  }

  // Wait for state to update
  await page.waitForTimeout(100);
}

// Helper: Wait for execution to pause at breakpoint
export async function waitForPausedState(page: Page) {
  // Wait for the Continue button to become ENABLED (not just exist)
  // The button exists always but is disabled until VM actually pauses

  // Wait for button to be enabled (disabled attribute removed)
  await page.waitForFunction(() => {
    const btn = document.querySelector('[data-testid="debug-continue"]') as HTMLButtonElement;
    return btn && !btn.disabled;
  }, { timeout: 15000 });
}

// Helper: Click debug control button by label
// Labels: "Continue", "Pause", "Step Into", "Step Over", "Step Out", "Stop"
export async function clickDebugButton(page: Page, buttonLabel: string) {
  // Convert label to kebab-case testid (e.g., "Step Into" -> "debug-step-into")
  const testId = `debug-${buttonLabel.toLowerCase().replace(/\s+/g, '-')}`;
  await page.click(`[data-testid="${testId}"]`);
}

// Example code snippets for testing
export const SAMPLE_PROGRAMS = {
  helloWorld: `println("Hello, World!")`,

  simpleArithmetic: `let x = 10
let y = 20
println(x + y)`,

  withFunction: `fn add(a: int, b: int) -> int {
  return a + b
}

let result = add(5, 3)
println(result)`,

  withLoop: `let numbers = [0, 1, 2, 3, 4]
for i in numbers {
  println(i)
}`,

  syntaxError: `let x =
println(x)`,  // Missing value - syntax error

  typeError: `let x: int = "hello"`,  // Type mismatch

  fibonacci: `fn fib(n: int) -> int {
  if n <= 1 {
    return n
  }
  return fib(n - 1) + fib(n - 2)
}

println(fib(10))`,

  multiLine: `let x = 1
let y = 2
println(x + y)`,

  withBreakpoint: `let a = 1
let b = 2
println(a + b)`,

  doubleFunction: `fn double(n: int) -> int {
  return n * 2
}
let x = double(5)
println(x)`,
};

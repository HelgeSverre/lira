# Lira Playground Core Tests

**Seed:** `e2e/seed.spec.ts`

This test plan covers the core functionality of the Lira Playground application.

## Test Selectors

The following `data-testid` selectors are available for E2E tests:

- **Buttons:** `run-button`, `compile-button`
- **Sample Selector:** `sample-selector`, `sample-select`
- **Output Panel:** `output-panel`, `output-tabs`, `output-console`, `output-content`
- **Tabs:** `tab-output`, `tab-debug`, `tab-vm`, `tab-fibers`, `tab-channels`
- **Console States:** `console-placeholder`, `console-line`, `console-error`, `console-finished`
- **AST Panel:** `ast-panel`, `ast-content`, `ast-placeholder`, `ast-errors`, `copy-ast-json`
- **Editor:** `editor-panel`, `editor-content`
- **Status Bar:** `status-bar`, `status-indicator`, `cursor-position`

---

## 1. Editor Basic Operations

### 1.1 Load Playground Interface
**Steps:**
1. Navigate to the playground URL
2. Wait for the page to fully load

**Expected Results:**
- Monaco editor is visible and ready for input
- Run button is visible and enabled
- Output panel is visible
- AST panel section is present

### 1.2 Type Code in Editor
**Steps:**
1. Click on the Monaco editor area
2. Type `println("Hello")`

**Expected Results:**
- Code appears in the editor
- Syntax highlighting is applied (keywords colored differently)

### 1.3 Clear Editor Content
**Steps:**
1. Type some code in the editor
2. Select all text (Ctrl+A)
3. Delete selected text

**Expected Results:**
- Editor is empty
- No compilation errors shown

---

## 2. Code Compilation

### 2.1 Compile Valid Code
**Steps:**
1. Enter valid Lira code: `let x = 42`
2. Click the Run button

**Expected Results:**
- No error messages appear
- AST tree is populated with Program node
- Execution completes without errors

### 2.2 Compile Code with Syntax Error
**Steps:**
1. Enter invalid code: `let x =`
2. Click the Run button

**Expected Results:**
- Error message is displayed
- Error indicator appears in the editor
- Button shows error state or returns to ready state

### 2.3 Compile Code with Type Error
**Steps:**
1. Enter code with type mismatch: `let x: int = "hello"`
2. Click the Run button

**Expected Results:**
- Type error message is displayed
- Error mentions type mismatch

---

## 3. Code Execution

### 3.1 Run Hello World
**Steps:**
1. Enter code: `println("Hello, World!")`
2. Click the Run button
3. Wait for execution to complete

**Expected Results:**
- Output panel shows "Hello, World!"
- Execution completes successfully
- Exit code is 0

### 3.2 Run Arithmetic Program
**Steps:**
1. Enter code:
   ```
   let a = 10
   let b = 20
   println(a + b)
   ```
2. Click the Run button

**Expected Results:**
- Output shows "30"
- No errors occur

### 3.3 Run Function Call
**Steps:**
1. Enter code:
   ```
   fn add(x: int, y: int) -> int {
     return x + y
   }
   println(add(5, 3))
   ```
2. Click the Run button

**Expected Results:**
- Output shows "8"
- Function is correctly executed

### 3.4 Run Program with Loop
**Steps:**
1. Enter code:
   ```
   let numbers = [0, 1, 2]
   for i in numbers {
     println(i)
   }
   ```
2. Click the Run button

**Expected Results:**
- Output shows "0", "1", "2" on separate lines

### 3.5 Stop Running Program
**Steps:**
1. Enter an infinite loop or long-running code
2. Click Run
3. Click Stop button

**Expected Results:**
- Execution stops
- Stop button becomes Run button again
- Output shows what was printed before stop

---

## 4. Output Panel

### 4.1 View Output Tab
**Steps:**
1. Run a program that produces output
2. Click the "Output" tab

**Expected Results:**
- Output console shows program output
- Text is displayed correctly

### 4.2 View Debug Tab
**Steps:**
1. Click the "Debug" tab

**Expected Results:**
- Debug panel is shown
- Shows "Not debugging" message when not in debug mode

### 4.3 View VM Tab
**Steps:**
1. Run a program
2. Click the "VM" tab

**Expected Results:**
- VM inspector panel is shown
- Shows VM state information

### 4.4 Clear Output Between Runs
**Steps:**
1. Run `println("First")`
2. Wait for output
3. Change code to `println("Second")`
4. Run again

**Expected Results:**
- Output shows only "Second" (previous output cleared)

---

## 5. AST Panel

### 5.1 View AST Tree
**Steps:**
1. Enter code: `let x = 42`
2. Run the code

**Expected Results:**
- AST panel shows tree structure
- Root node is "Program"
- VarDecl node is present

### 5.2 Expand AST Nodes
**Steps:**
1. Run code with nested structure
2. Click on collapsed AST nodes

**Expected Results:**
- Nodes expand to show children
- Child nodes are properly indented

### 5.3 AST Shows Function Structure
**Steps:**
1. Enter code with function:
   ```
   fn greet(name: string) {
     println(name)
   }
   ```
2. Run the code

**Expected Results:**
- AST shows FnDecl node
- Parameters are visible
- Body block is present

---

## 6. Error Handling

### 6.1 Display Runtime Error
**Steps:**
1. Enter code that causes runtime error (e.g., division by zero if supported)
2. Run the code

**Expected Results:**
- Runtime error message is displayed
- Error location is highlighted if available

### 6.2 Recover from Error State
**Steps:**
1. Run code that produces an error
2. Fix the code
3. Run again

**Expected Results:**
- Error state is cleared
- New run executes successfully

---

## 7. Sample Programs

### 7.1 Load Sample Program
**Steps:**
1. Find sample program selector (if available)
2. Select a sample program

**Expected Results:**
- Editor content changes to sample code
- Sample is valid Lira code

### 7.2 Run Sample Program
**Steps:**
1. Load a sample program
2. Click Run

**Expected Results:**
- Sample executes without errors
- Output is produced (if applicable)

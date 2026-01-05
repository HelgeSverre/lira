# Lira Playground Debugging Tests

**Seed:** `e2e/seed.spec.ts`

This test plan covers the debugging functionality of the Lira Playground.

## Test Selectors

The following `data-testid` selectors are available for E2E tests:

- **Debug Controls:** `debug-controls`, `debug-continue`, `debug-pause`, `debug-step-into`, `debug-step-over`, `debug-step-out`, `debug-stop`
- **Run Button:** `run-button` (with `data-state` attribute)
- **Output:** `output-console`, `console-paused`, `console-error`
- **Debug Panel:** `debug-panel`, `debug-empty`, `debug-locals`, `debug-stack`, `debug-callstack`
- **Tabs:** `tab-output`, `tab-debug`, `tab-vm`

---

## 1. Breakpoint Management

### 1.1 Set Breakpoint on Line
**Steps:**
1. Enter multi-line code:
   ```
   let x = 1
   let y = 2
   println(x + y)
   ```
2. Click on the gutter (line number area) for line 2

**Expected Results:**
- Breakpoint indicator appears on line 2
- Breakpoint is visually marked (red dot or similar)

### 1.2 Remove Breakpoint
**Steps:**
1. Set a breakpoint on a line
2. Click on the same breakpoint indicator

**Expected Results:**
- Breakpoint is removed
- Visual indicator disappears

### 1.3 Set Multiple Breakpoints
**Steps:**
1. Enter code with multiple lines
2. Set breakpoints on lines 1 and 3

**Expected Results:**
- Both breakpoints are shown
- Both lines have breakpoint indicators

---

## 2. Debug Execution

### 2.1 Run to Breakpoint
**Steps:**
1. Enter code:
   ```
   let x = 1
   let y = 2
   println(x + y)
   ```
2. Set breakpoint on line 2
3. Click Run

**Expected Results:**
- Execution pauses at line 2
- Current line is highlighted
- Debug panel shows "Paused" state
- Run button changes to show paused state

### 2.2 View Locals at Breakpoint
**Steps:**
1. Set breakpoint on line 2 of the code above
2. Run to breakpoint
3. View Debug tab

**Expected Results:**
- Debug panel shows local variables
- Variable `x` is shown with value `1`

### 2.3 View Stack at Breakpoint
**Steps:**
1. Run to a breakpoint
2. View Debug tab stack section

**Expected Results:**
- Stack values are displayed
- Values are shown in readable format

---

## 3. Stepping Operations

### 3.1 Step Into
**Steps:**
1. Enter code with function call:
   ```
   fn double(n: int) -> int {
     return n * 2
   }
   let x = double(5)
   println(x)
   ```
2. Set breakpoint on the function call line
3. Run to breakpoint
4. Click "Step Into" button

**Expected Results:**
- Execution moves into the function
- Current line shows first line of function body
- Debug panel updates with function context

### 3.2 Step Over
**Steps:**
1. Use same code as 3.1
2. Run to breakpoint on function call
3. Click "Step Over" button

**Expected Results:**
- Execution completes the function call
- Current line moves to next line after call
- Function is not stepped into

### 3.3 Step Out
**Steps:**
1. Step into a function
2. Click "Step Out" button

**Expected Results:**
- Execution continues until function returns
- Current line is back in caller context

### 3.4 Continue Execution
**Steps:**
1. Set breakpoint on line 2
2. Run to breakpoint
3. Click Continue button

**Expected Results:**
- Execution continues to next breakpoint or completion
- If no more breakpoints, program finishes

---

## 4. Debug Controls

### 4.1 Pause Running Program
**Steps:**
1. Enter long-running code (loop with many iterations)
2. Click Run
3. Click Pause button

**Expected Results:**
- Execution pauses
- Current line is highlighted
- Pause button changes to Continue/Play

### 4.2 Stop Debug Session
**Steps:**
1. Start debugging with breakpoints
2. Pause at a breakpoint
3. Click Stop/Reset button

**Expected Results:**
- Debug session ends
- Highlighted line is cleared
- Debug panel returns to initial state

### 4.3 Debug Controls State
**Steps:**
1. Check debug controls before running
2. Run to breakpoint
3. Check debug controls while paused

**Expected Results:**
- Before run: Step buttons are disabled
- While paused: Step buttons are enabled
- Continue/Pause button reflects current state

---

## 5. Debug Panel Display

### 5.1 Show Execution State
**Steps:**
1. Run to a breakpoint
2. View Debug tab

**Expected Results:**
- Shows current execution state (Paused, Running, etc.)
- Shows current line and column
- Shows instruction pointer (IP)

### 5.2 Show Local Variables
**Steps:**
1. Enter code with variables at different scopes
2. Run to breakpoint after variable assignments

**Expected Results:**
- All in-scope local variables are shown
- Variable names, types, and values are displayed

### 5.3 Show Call Stack
**Steps:**
1. Enter code with nested function calls
2. Set breakpoint inside innermost function
3. Run to breakpoint

**Expected Results:**
- Call stack shows all active frames
- Most recent frame is at top
- Frame names indicate function names

### 5.4 Empty State Display
**Steps:**
1. Navigate to playground
2. Click Debug tab without running

**Expected Results:**
- Shows "Not debugging" message
- Hint text suggests setting breakpoints

---

## 6. Breakpoint Behavior

### 6.1 Breakpoint in Function
**Steps:**
1. Set breakpoint inside a function
2. Call the function from main code

**Expected Results:**
- Execution pauses when function is called
- Breakpoint triggers on each call

### 6.2 Breakpoint in Loop
**Steps:**
1. Enter loop:
   ```
   let nums = [0, 1, 2]
   for i in nums {
     println(i)
   }
   ```
2. Set breakpoint on println line (line 3)
3. Run and continue through iterations

**Expected Results:**
- Breakpoint triggers on each iteration
- `i` variable updates on each pause

### 6.3 Breakpoint Persists Across Runs
**Steps:**
1. Set breakpoints
2. Run the code
3. Stop execution
4. Run again without changing breakpoints

**Expected Results:**
- Breakpoints remain set
- Second run pauses at same breakpoints

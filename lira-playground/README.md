# Lira Playground

A web-based playground for writing and running Lira code in the browser with full debugging support.

## Architecture

- **Frontend**: React + TypeScript + Vite app with Monaco editor
- **Backend**: Rust/Axum web server that compiles and executes Lira code

## Quick Start

### Backend

```bash
# From the project root
cargo build --release --package lira-playground

# Start the server (default port 3001)
./target/release/lira-playground

# Or with a custom port
PORT=3010 ./target/release/lira-playground
```

### Frontend

```bash
cd lira-playground/frontend

# Install dependencies
pnpm install

# Development mode (uses localhost:3001 by default)
pnpm dev

# Build for production
pnpm build
```

## Features

### Code Editing
- Monaco editor with Lira syntax highlighting
- Breakpoint support (click gutter to toggle)
- Cursor position display
- Sample program selector

### Execution
- Simple run mode (HTTP-based)
- Debug mode with breakpoints (WebSocket-based)
- Real-time output streaming

### Debugging
- Set breakpoints on any line
- Step Into, Step Over, Step Out controls
- Continue/Pause execution
- Local variable inspection
- Stack inspection
- Call stack display

### AST Visualization
- View parsed AST tree
- Copy AST as JSON
- Expandable/collapsible nodes

## API Endpoints

### `GET /health`
Health check endpoint. Returns `OK` if the server is running.

### `POST /api/compile`
Compile Lira source code and return AST + errors.

**Request:**
```json
{
  "source": "println(42)"
}
```

**Response:**
```json
{
  "success": true,
  "ast": { ... },
  "errors": [],
  "bytecodeSize": 91,
  "compileTimeMs": 1
}
```

### `POST /api/run`
Compile and execute Lira source code.

**Request:**
```json
{
  "source": "println(\"Hello, World!\")"
}
```

**Response:**
```json
{
  "success": true,
  "output": ["Hello, World!"],
  "exitCode": 0,
  "errors": [],
  "executionTimeMs": 1
}
```

### `POST /api/check`
Type check source code without generating bytecode.

### `GET /ws`
WebSocket endpoint for real-time debugging.

#### Client Messages
| Type | Description |
|------|-------------|
| `debug` | Start debug session with source and breakpoints |
| `continue` | Continue execution to next breakpoint |
| `stepInto` | Step into function call |
| `stepOver` | Step over function call |
| `stepOut` | Step out of current function |
| `pause` | Pause running execution |
| `stop` | Stop debug session |
| `setBreakpoints` | Update breakpoint locations |

#### Server Messages
| Type | Description |
|------|-------------|
| `compileSuccess` | Compilation succeeded, includes AST |
| `compileError` | Compilation failed with errors |
| `breakpointHit` | Execution paused at breakpoint |
| `stepCompleted` | Step operation completed |
| `output` | Program output line |
| `finished` | Execution completed |
| `runtimeError` | Runtime error occurred |
| `vmStateUpdate` | Debug state update (locals, stack) |

## Configuration

### Environment Variables

- `PORT` - Server port (default: 3001)
- `VITE_API_URL` - Frontend API URL (default: http://localhost:3001)

## Development

### Running Tests

```bash
# Run backend tests
cargo test --package lira-playground

# Run frontend E2E tests
cd lira-playground/frontend && npx playwright test

# Run E2E tests with UI
cd lira-playground/frontend && npx playwright test --ui

# Run E2E tests headed (visible browser)
cd lira-playground/frontend && npx playwright test --headed
```

### Code Structure

```
lira-playground/
├── backend/
│   └── src/
│       ├── main.rs         # Server entry point
│       ├── handlers.rs     # HTTP/WebSocket handlers
│       ├── protocol.rs     # Protocol type definitions
│       └── vm_thread.rs    # VM execution thread management
└── frontend/
    ├── src/
    │   ├── api/            # Backend API client
    │   ├── components/     # React components
    │   ├── stores/         # Zustand state stores
    │   └── types/          # TypeScript type definitions
    └── e2e/
        ├── helpers.ts      # E2E test utilities
        └── *.spec.ts       # Playwright test files
```

## Testing with data-testid Attributes

The UI components expose `data-testid` attributes for reliable E2E testing.

### Header Controls

| Element | data-testid |
|---------|-------------|
| Run button | `run-button` |
| Compile button | `compile-button` |
| Debug controls container | `debug-controls` |
| Continue button | `debug-continue` |
| Pause button | `debug-pause` |
| Step Into button | `debug-step-into` |
| Step Over button | `debug-step-over` |
| Step Out button | `debug-step-out` |
| Stop button | `debug-stop` |
| Sample selector | `sample-selector` |
| Sample dropdown | `sample-select` |

### Output Panel

| Element | data-testid |
|---------|-------------|
| Output panel | `output-panel` |
| Tab bar | `output-tabs` |
| Output tab | `tab-output` |
| Debug tab | `tab-debug` |
| VM tab | `tab-vm` |
| Fibers tab | `tab-fibers` |
| Channels tab | `tab-channels` |
| Output console | `output-console` |
| Console placeholder | `console-placeholder` |
| Console line | `console-line` |
| Console error | `console-error` |
| Console paused | `console-paused` |
| Console finished | `console-finished` |

### Debug Panel

| Element | data-testid |
|---------|-------------|
| Debug panel | `debug-panel` |
| Empty state | `debug-empty` |
| Execution section | `debug-execution` |
| Locals section | `debug-locals` |
| Local variable | `local-var` |
| Stack section | `debug-stack` |
| Stack value | `stack-value` |
| Call stack section | `debug-callstack` |
| Call frame | `call-frame` |

### AST Panel

| Element | data-testid |
|---------|-------------|
| AST panel | `ast-panel` |
| AST content | `ast-content` |
| Statement count | `ast-statement-count` |
| Copy JSON button | `copy-ast-json` |
| Placeholder | `ast-placeholder` |
| Compiling state | `ast-compiling` |
| Errors container | `ast-errors` |
| Error item | `ast-error` |

### Editor & Status

| Element | data-testid |
|---------|-------------|
| Editor panel | `editor-panel` |
| Editor content | `editor-content` |
| Status bar | `status-bar` |
| Status indicator | `status-indicator` |
| Cursor position | `cursor-position` |
| Language indicator | `language-indicator` |
| Fiber count | `fiber-count` |

### Data Attributes

Some elements include additional data attributes for state:

- `data-state` - Current state (e.g., `idle`, `running`, `paused`, `compiling`)
- `data-status` - Execution status
- `data-active` - Whether a tab is active
- `data-name` - Variable name (for locals)

### Example E2E Test

```typescript
import { test, expect } from '@playwright/test';

test('run program and check output', async ({ page }) => {
  await page.goto('/');

  // Click run button
  await page.click('[data-testid="run-button"]');

  // Wait for output
  await expect(page.locator('[data-testid="output-console"]'))
    .toContainText('Hello, World!');

  // Check status
  await expect(page.locator('[data-testid="status-indicator"]'))
    .toHaveAttribute('data-status', 'finished');
});

test('debug with breakpoint', async ({ page }) => {
  await page.goto('/');

  // Set breakpoint via store
  await page.evaluate(() => {
    (window as any).__EDITOR_STORE__.getState().toggleBreakpoint(2);
  });

  // Run - should pause at breakpoint
  await page.click('[data-testid="run-button"]');

  // Wait for continue button to be enabled
  await page.waitForFunction(() => {
    const btn = document.querySelector('[data-testid="debug-continue"]');
    return btn && !btn.disabled;
  });

  // Continue execution
  await page.click('[data-testid="debug-continue"]');
});
```

# Lira Playground

A web-based playground for writing and running Lira code in the browser.

## Architecture

- **Frontend**: React + TypeScript + Vite app with CodeMirror editor
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
WebSocket endpoint for real-time execution (streaming output, debugging, etc.)

## Configuration

### Environment Variables

- `PORT` - Server port (default: 3001)
- `VITE_API_URL` - Frontend API URL (default: http://localhost:3001)

## Development

### Running Tests

```bash
# Run backend tests
cargo test --package lira-playground

# Run frontend tests
cd lira-playground/frontend && pnpm test
```

### Code Structure

```
lira-playground/
├── backend/
│   └── src/
│       ├── main.rs         # Server entry point
│       ├── handlers.rs     # HTTP/WebSocket handlers
│       ├── protocol.rs     # Protocol type definitions
│       └── session.rs      # WebSocket session management
└── frontend/
    └── src/
        ├── api/            # Backend API client
        ├── components/     # React components
        ├── stores/         # Zustand state stores
        └── types/          # TypeScript type definitions
```

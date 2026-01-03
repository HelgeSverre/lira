/**
 * Lira Playground API Client
 *
 * Handles communication with the backend server for compilation and execution.
 */

import type { Program } from '../types/ast';
import type { CompileError } from '../types/protocol';

// Use environment variable or empty string for same-origin requests (proxy handles in dev)
const API_BASE = import.meta.env.VITE_API_URL || '';

/**
 * Transform AST from backend format {type, value: {...fields}} to frontend format {type, ...fields}
 * This is needed because the backend uses serde's adjacently tagged enums.
 */
function normalizeAst(obj: unknown): unknown {
  if (obj === null || obj === undefined) {
    return obj;
  }

  if (Array.isArray(obj)) {
    return obj.map(normalizeAst);
  }

  if (typeof obj === 'object') {
    const record = obj as Record<string, unknown>;

    // Check if this is a tagged enum {type, value}
    if ('type' in record && 'value' in record && Object.keys(record).length === 2) {
      const value = record.value;
      if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
        // Flatten {type, value: {...fields}} to {type, ...fields}
        const normalized: Record<string, unknown> = { type: record.type };
        for (const [key, val] of Object.entries(value as Record<string, unknown>)) {
          normalized[key] = normalizeAst(val);
        }
        return normalized;
      } else {
        // For simple values like {type: "IntLiteral", value: 42}
        return { type: record.type, value: normalizeAst(value) };
      }
    }

    // Recursively normalize all object properties
    const result: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(record)) {
      result[key] = normalizeAst(val);
    }
    return result;
  }

  return obj;
}

export interface CompileResponse {
  success: boolean;
  ast: Program | null;
  errors: CompileError[];
  bytecodeSize: number;
  compileTimeMs: number;
}

export interface RunResponse {
  success: boolean;
  output: string[];
  exitCode: number | null;
  errors: CompileError[];
  executionTimeMs: number;
}

export interface CheckResponse {
  success: boolean;
  errors: CompileError[];
}

/**
 * Compile source code and get the AST
 */
export async function compile(source: string): Promise<CompileResponse> {
  const response = await fetch(`${API_BASE}/api/compile`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ source }),
  });

  if (!response.ok) {
    throw new Error(`Compilation request failed: ${response.statusText}`);
  }

  const result = await response.json();

  // Normalize AST from backend format to frontend format
  if (result.ast) {
    result.ast = normalizeAst(result.ast) as Program;
  }

  return result;
}

/**
 * Compile and run source code
 * @param source - The source code to run
 * @param breakpoints - Optional array of line numbers (1-based) where to pause
 */
export async function run(source: string, breakpoints: number[] = []): Promise<RunResponse> {
  const response = await fetch(`${API_BASE}/api/run`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ source, breakpoints }),
  });

  if (!response.ok) {
    throw new Error(`Run request failed: ${response.statusText}`);
  }

  return response.json();
}

/**
 * Type check source code without generating bytecode
 */
export async function check(source: string): Promise<CheckResponse> {
  const response = await fetch(`${API_BASE}/api/check`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ source }),
  });

  if (!response.ok) {
    throw new Error(`Check request failed: ${response.statusText}`);
  }

  return response.json();
}

/**
 * Create a WebSocket connection for real-time execution
 */
export function createWebSocket(): WebSocket {
  let wsUrl: string;
  if (API_BASE) {
    // External API base - convert http to ws
    wsUrl = API_BASE.replace(/^http/, 'ws') + '/ws';
  } else {
    // Same-origin - construct from current location
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    wsUrl = `${protocol}//${window.location.host}/ws`;
  }
  return new WebSocket(wsUrl);
}

/**
 * Check if the backend is available
 */
export async function healthCheck(): Promise<boolean> {
  try {
    const response = await fetch(`${API_BASE}/health`);
    return response.ok;
  } catch {
    return false;
  }
}

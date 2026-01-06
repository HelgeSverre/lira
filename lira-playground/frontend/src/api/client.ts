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
 * Convert snake_case to camelCase
 */
function snakeToCamel(str: string): string {
  return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

/**
 * Map of AST type names to the key name their simple value should use.
 * For example, Identifier's value should be stored as "name", not "value".
 */
const VALUE_KEY_MAPPING: Record<string, string> = {
  'Identifier': 'name',
  'Named': 'name',  // TypeExpr Named type
  // Literals keep their value as "value" (no mapping needed)
};

/**
 * Transform AST from backend format {type, value: {...fields}} to frontend format {type, ...fields}
 * Also converts snake_case keys to camelCase.
 * This is needed because the backend uses serde's adjacently tagged enums and snake_case.
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
      const typeName = record.type as string;

      if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
        // Flatten {type, value: {...fields}} to {type, ...fields}
        const normalized: Record<string, unknown> = { type: typeName };
        for (const [key, val] of Object.entries(value as Record<string, unknown>)) {
          normalized[snakeToCamel(key)] = normalizeAst(val);
        }
        return normalized;
      } else {
        // For simple values like {type: "Identifier", value: "println"} or {type: "IntLiteral", value: 42}
        // Map to the appropriate key name based on type
        const keyName = VALUE_KEY_MAPPING[typeName] || 'value';
        return { type: typeName, [keyName]: normalizeAst(value) };
      }
    }

    // Recursively normalize all object properties, converting snake_case to camelCase
    const result: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(record)) {
      result[snakeToCamel(key)] = normalizeAst(val);
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

export interface BreakpointInfo {
  line: number;
  column: number;
  ip: number;
}

export interface RunResponse {
  success: boolean;
  output: string[];
  exitCode: number | null;
  errors: CompileError[];
  executionTimeMs: number;
  /** If execution paused at a breakpoint, this contains the location */
  breakpoint?: BreakpointInfo;
}

export interface CheckResponse {
  success: boolean;
  errors: CompileError[];
}

export type StepType = 'into' | 'over' | 'out' | 'continue';

export interface StepRequest {
  source: string;
  currentLine: number;
  stepType: StepType;
  breakpoints: number[];
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
 * Step through code (for debugging)
 * Re-runs the code with a temporary breakpoint at the next line
 */
export async function step(request: StepRequest): Promise<RunResponse> {
  const response = await fetch(`${API_BASE}/api/step`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    throw new Error(`Step request failed: ${response.statusText}`);
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

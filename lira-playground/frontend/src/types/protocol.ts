/**
 * WebSocket Protocol Type Definitions
 * For communication between frontend and Rust backend
 */

import type { Program } from './ast';
import type { FiberId, ChannelId } from './vm';

/** Messages sent from client to server */
export type ClientMessage =
  | { type: 'check'; source: string }
  | { type: 'run'; source: string }
  | { type: 'debug'; source: string }
  | { type: 'setBreakpoints'; breakpoints: number[] }
  | { type: 'continue' }
  | { type: 'stepInstruction' }
  | { type: 'stepLine' }
  | { type: 'stepInto' }
  | { type: 'stepOut' }
  | { type: 'inspectVariable'; name: string }
  | { type: 'inspectLocals' }
  | { type: 'inspectStack' }
  | { type: 'pause' }
  | { type: 'stop' }
  | { type: 'getAst'; source: string }
  | { type: 'ping' };

/** Messages sent from server to client */
export type ServerMessage =
  | { type: 'compileSuccess'; ast: Program | null; bytecodeSize: number }
  | { type: 'compileError'; errors: CompileError[] }
  | { type: 'output'; text: string }
  | { type: 'finished'; exitCode: number; durationMs: number }
  | { type: 'runtimeError'; message: string; location: SourceLocation | null }
  | { type: 'breakpointHit'; line: number; column: number; ip: number }
  | { type: 'paused'; line: number; column: number; ip: number }
  | { type: 'stepCompleted'; line: number; column: number; ip: number }
  | { type: 'variableValue'; name: string; value: string; typeName: string }
  | { type: 'locals'; locals: VariableInfo[] }
  | { type: 'stack'; stack: string[] }
  | { type: 'ast'; ast: Program }
  | { type: 'vmStateUpdate'; state: DebugVmState }
  | { type: 'fiberSpawned'; fiber: FiberInfo }
  | { type: 'fiberStateChanged'; fiberId: FiberId; newState: string }
  | { type: 'channelCreated'; channel: ChannelInfo }
  | { type: 'channelMessage'; channelId: ChannelId; operation: 'send' | 'receive'; value: string }
  | { type: 'pong' }
  | { type: 'timeoutWarning'; secondsRemaining: number }
  | { type: 'stopped' };

/** Compilation error */
export interface CompileError {
  message: string;
  line: number | null;
  column: number | null;
  severity: ErrorSeverity;
}

/** Error severity levels */
export type ErrorSeverity = 'error' | 'warning' | 'hint';

/** Source location */
export interface SourceLocation {
  line: number;
  column: number;
  file: string | null;
}

/** Variable info for inspection */
export interface VariableInfo {
  name: string;
  value: string;
  typeName: string;
}

/** Call frame info for stack trace */
export interface CallFrameInfo {
  functionName: string | null;
  line: number | null;
  ip: number;
}

/** Debug VM state for vmStateUpdate messages */
export interface DebugVmState {
  executionState: string;
  ip: number;
  line: number | null;
  column: number | null;
  stack: string[];
  locals: VariableInfo[];
  callStack: string[];
  output: string[];
}

/** Fiber info for state updates */
export interface FiberInfo {
  id: FiberId;
  state: string;
  ip: number;
}

/** Channel info for state updates */
export interface ChannelInfo {
  id: ChannelId;
  capacity: number;
  bufferSize: number;
  closed: boolean;
}

/** Helper to create client messages */
export const createClientMessage = {
  check: (source: string): ClientMessage => ({ type: 'check', source }),
  run: (source: string): ClientMessage => ({ type: 'run', source }),
  debug: (source: string): ClientMessage => ({ type: 'debug', source }),
  setBreakpoints: (breakpoints: number[]): ClientMessage => ({ type: 'setBreakpoints', breakpoints }),
  continue: (): ClientMessage => ({ type: 'continue' }),
  stepInstruction: (): ClientMessage => ({ type: 'stepInstruction' }),
  stepLine: (): ClientMessage => ({ type: 'stepLine' }),
  stepInto: (): ClientMessage => ({ type: 'stepInto' }),
  stepOut: (): ClientMessage => ({ type: 'stepOut' }),
  inspectVariable: (name: string): ClientMessage => ({ type: 'inspectVariable', name }),
  inspectLocals: (): ClientMessage => ({ type: 'inspectLocals' }),
  inspectStack: (): ClientMessage => ({ type: 'inspectStack' }),
  pause: (): ClientMessage => ({ type: 'pause' }),
  stop: (): ClientMessage => ({ type: 'stop' }),
  getAst: (source: string): ClientMessage => ({ type: 'getAst', source }),
  ping: (): ClientMessage => ({ type: 'ping' }),
};

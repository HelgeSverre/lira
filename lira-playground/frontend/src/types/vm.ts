/**
 * Lira VM State Type Definitions
 * Mirrors crates/liravm/src/value.rs and fiber.rs
 */

/** Fiber ID type */
export type FiberId = number;

/** Channel ID type */
export type ChannelId = number;

/** Value types in the VM */
export type Value =
  | { type: 'Null' }
  | { type: 'Bool'; value: boolean }
  | { type: 'Int'; value: number }
  | { type: 'Float'; value: number }
  | { type: 'String'; value: string }
  | { type: 'Array'; elements: Value[] }
  | { type: 'Tuple'; elements: Value[] }
  | { type: 'Object'; fields: Record<string, Value> }
  | { type: 'Function'; codeOffset: number }
  | { type: 'Closure'; codeOffset: number; captures: Value[] }
  | { type: 'Fiber'; id: FiberId }
  | { type: 'Channel'; id: ChannelId };

/** Fiber state */
export type FiberState =
  | { type: 'Ready' }
  | { type: 'Running' }
  | { type: 'BlockedReceive'; channelId: ChannelId }
  | { type: 'BlockedSend'; channelId: ChannelId }
  | { type: 'BlockedSelect' }
  | { type: 'Yielded' }
  | { type: 'Finished' }
  | { type: 'Failed'; error: string };

/** Call frame within a fiber */
export interface FiberCallFrame {
  returnAddr: number;
  localsBase: number;
  functionName?: string;
}

/** A fiber (green thread) */
export interface Fiber {
  id: FiberId;
  state: FiberState;
  ip: number;
  stack: Value[];
  locals: Value[];
  callStack: FiberCallFrame[];
  fiberLocals: Record<string, Value>;
  result: Value | null;
}

/** Channel for inter-fiber communication */
export interface Channel {
  id: ChannelId;
  buffer: Value[];
  capacity: number;
  receivers: FiberId[];
  senders: { fiberId: FiberId; value: Value }[];
  closed: boolean;
}

/** Complete VM state for inspection */
export interface VmState {
  fibers: Record<FiberId, Fiber>;
  channels: Record<ChannelId, Channel>;
  currentFiberId: FiberId | null;
  readyQueue: FiberId[];
  output: string[];
  exitCode: number | null;
}

/** Debug info for bytecode */
export interface DebugInfo {
  sourceFile: string | null;
  lineTable: LineInfo[];
  localVars: LocalVarInfo[];
}

/** Line number mapping */
export interface LineInfo {
  startOffset: number;
  endOffset: number;
  line: number;
  column: number;
}

/** Local variable info */
export interface LocalVarInfo {
  name: string;
  slot: number;
  startOffset: number;
  endOffset: number;
  varType: string;
}

/** Function info in bytecode */
export interface FunctionInfo {
  name: string;
  codeOffset: number;
  paramCount: number;
  localCount: number;
}

/** Bytecode program structure */
export interface BytecodeProgram {
  constants: Value[];
  code: number[];
  entryPoint: number;
  functions: FunctionInfo[];
  debugInfo: DebugInfo;
  sourceFile: string | null;
}

/** Execution status */
export type ExecutionStatus = 'idle' | 'compiling' | 'running' | 'paused' | 'finished' | 'error';

/** Helper functions */

/** Format a value for display */
export function formatValue(value: Value): string {
  switch (value.type) {
    case 'Null': return 'null';
    case 'Bool': return String(value.value);
    case 'Int': return String(value.value);
    case 'Float': return String(value.value);
    case 'String': return `"${value.value}"`;
    case 'Array': return `[${value.elements.map(formatValue).join(', ')}]`;
    case 'Tuple': {
      const elements = value.elements.map(formatValue).join(', ');
      return `(${elements}${value.elements.length === 1 ? ',' : ''})`;
    }
    case 'Object': {
      const fields = Object.entries(value.fields)
        .map(([k, v]) => `${k}: ${formatValue(v)}`)
        .join(', ');
      return `{${fields}}`;
    }
    case 'Function': return `<function@${value.codeOffset}>`;
    case 'Closure': return `<closure@${value.codeOffset}>`;
    case 'Fiber': return `<fiber#${value.id}>`;
    case 'Channel': return `<channel#${value.id}>`;
  }
}

/** Get short type name for a value */
export function getValueTypeName(value: Value): string {
  switch (value.type) {
    case 'Null': return 'null';
    case 'Bool': return 'bool';
    case 'Int': return 'int';
    case 'Float': return 'float';
    case 'String': return 'string';
    case 'Array': return 'array';
    case 'Tuple': return 'tuple';
    case 'Object': return 'object';
    case 'Function': return 'function';
    case 'Closure': return 'closure';
    case 'Fiber': return 'fiber';
    case 'Channel': return 'channel';
  }
}

/** Get fiber state display name */
export function getFiberStateName(state: FiberState): string {
  switch (state.type) {
    case 'Ready': return 'Ready';
    case 'Running': return 'Running';
    case 'BlockedReceive': return `Blocked (recv ch#${state.channelId})`;
    case 'BlockedSend': return `Blocked (send ch#${state.channelId})`;
    case 'BlockedSelect': return 'Blocked (select)';
    case 'Yielded': return 'Yielded';
    case 'Finished': return 'Finished';
    case 'Failed': return `Failed: ${state.error}`;
  }
}

/** Get fiber state CSS class */
export function getFiberStateClass(state: FiberState): string {
  switch (state.type) {
    case 'Ready': return 'ready';
    case 'Running': return 'running';
    case 'BlockedReceive':
    case 'BlockedSend':
    case 'BlockedSelect':
    case 'Yielded': return 'blocked';
    case 'Finished': return 'finished';
    case 'Failed': return 'failed';
  }
}

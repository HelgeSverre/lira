/**
 * WebSocket Store
 *
 * Manages WebSocket connection for real-time debugging.
 * Handles sending client messages and dispatching server messages to other stores.
 */

import { create } from 'zustand';
import { createWebSocket } from '../api/client';
import type { ClientMessage, ServerMessage, DebugVmState, VariableInfo, ValueJson } from '../types/protocol';
import { useVmStore } from './vmStore';
import { useEditorStore } from './editorStore';
import { useCompilerStore } from './compilerStore';

export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface WebSocketStoreState {
  /** WebSocket connection status */
  connectionStatus: ConnectionStatus;
  /** WebSocket instance */
  socket: WebSocket | null;
  /** Debug state from VM */
  debugState: DebugVmState | null;
  /** Current locals for variable inspection */
  locals: VariableInfo[];
  /** Current stack values */
  stack: ValueJson[];
  /** Error message if connection failed */
  connectionError: string | null;

  // Actions
  connect: () => void;
  disconnect: () => void;
  send: (message: ClientMessage) => void;
  /** Start a debug session with source code */
  startDebug: (source: string, breakpoints: number[], fiberMode?: boolean) => void;
  /** Continue execution */
  continueExecution: () => void;
  /** Step operations */
  stepLine: () => void;
  stepInto: () => void;
  stepOver: () => void;
  stepOut: () => void;
  stepInstruction: () => void;
  /** Pause execution */
  pause: () => void;
  /** Stop execution */
  stop: () => void;
  /** Set breakpoints */
  setBreakpoints: (breakpoints: number[]) => void;
}

export const useWebSocketStore = create<WebSocketStoreState>((set, get) => ({
  connectionStatus: 'disconnected',
  socket: null,
  debugState: null,
  locals: [],
  stack: [],
  connectionError: null,

  connect: () => {
    const { socket, connectionStatus } = get();

    // Don't reconnect if already connected/connecting
    if (socket && (connectionStatus === 'connected' || connectionStatus === 'connecting')) {
      return;
    }

    set({ connectionStatus: 'connecting', connectionError: null });

    try {
      const ws = createWebSocket();

      ws.onopen = () => {
        set({ connectionStatus: 'connected', socket: ws });
        console.log('WebSocket connected');
      };

      ws.onclose = () => {
        set({ connectionStatus: 'disconnected', socket: null });
        console.log('WebSocket disconnected');
      };

      ws.onerror = (event) => {
        console.error('WebSocket error:', event);
        set({
          connectionStatus: 'error',
          connectionError: 'Failed to connect to server',
        });
      };

      ws.onmessage = (event) => {
        try {
          const message: ServerMessage = JSON.parse(event.data);
          handleServerMessage(message, set);
        } catch (error) {
          console.error('Failed to parse WebSocket message:', error);
        }
      };

      set({ socket: ws });
    } catch (error) {
      console.error('Failed to create WebSocket:', error);
      set({
        connectionStatus: 'error',
        connectionError: 'Failed to create WebSocket connection',
      });
    }
  },

  disconnect: () => {
    const { socket } = get();
    if (socket) {
      socket.close();
      set({ socket: null, connectionStatus: 'disconnected' });
    }
  },

  send: (message: ClientMessage) => {
    const { socket, connectionStatus } = get();
    if (socket && connectionStatus === 'connected') {
      socket.send(JSON.stringify(message));
    } else {
      console.warn('Cannot send message: WebSocket not connected');
    }
  },

  startDebug: (source: string, breakpoints: number[], fiberMode = false) => {
    const { send, connect, connectionStatus } = get();

    // Ensure we're connected
    if (connectionStatus !== 'connected') {
      connect();
      // Wait for connection, then send
      const checkAndSend = () => {
        const { connectionStatus: status, send: sendFn } = get();
        if (status === 'connected') {
          // Send debug message with breakpoints included (no race condition)
          sendFn({ type: 'debug', source, breakpoints, fiberMode });
        } else if (status === 'connecting') {
          setTimeout(checkAndSend, 100);
        }
      };
      setTimeout(checkAndSend, 100);
    } else {
      // Already connected - send debug message with breakpoints included
      send({ type: 'debug', source, breakpoints, fiberMode });
    }
  },

  continueExecution: () => {
    get().send({ type: 'continue' });
  },

  stepLine: () => {
    get().send({ type: 'stepLine' });
  },

  stepInto: () => {
    get().send({ type: 'stepInto' });
  },

  stepOver: () => {
    get().send({ type: 'stepOver' });
  },

  stepOut: () => {
    get().send({ type: 'stepOut' });
  },

  stepInstruction: () => {
    get().send({ type: 'stepInstruction' });
  },

  pause: () => {
    get().send({ type: 'pause' });
  },

  stop: () => {
    get().send({ type: 'stop' });
    // Reset VM store
    useVmStore.getState().reset();
    useEditorStore.getState().setHighlightedLine(null);
  },

  setBreakpoints: (breakpoints: number[]) => {
    get().send({ type: 'setBreakpoints', breakpoints });
  },
}));

/**
 * Handle incoming server messages
 */
function handleServerMessage(
  message: ServerMessage,
  set: (state: Partial<WebSocketStoreState>) => void
) {
  const vmStore = useVmStore.getState();
  const editorStore = useEditorStore.getState();
  const compilerStore = useCompilerStore.getState();

  switch (message.type) {
    case 'compileSuccess':
      vmStore.setRunning();
      if (message.ast) {
        compilerStore.setCompilationSuccess(message.ast);
      }
      break;

    case 'compileError':
      compilerStore.setCompilationError(message.errors);
      vmStore.setError(message.errors[0]?.message || 'Compilation failed');
      break;

    case 'output':
      vmStore.appendOutput(message.text);
      break;

    case 'finished':
      vmStore.setFinished(message.exitCode, message.durationMs);
      vmStore.setTimeoutWarning(null);
      editorStore.setHighlightedLine(null);
      break;

    case 'runtimeError':
      vmStore.setError(message.message);
      vmStore.setTimeoutWarning(null);
      if (message.location) {
        editorStore.setHighlightedLine(message.location.line);
      }
      break;

    case 'breakpointHit':
      vmStore.setPaused();
      editorStore.setHighlightedLine(message.line);
      break;

    case 'paused':
      vmStore.setPaused();
      editorStore.setHighlightedLine(message.line);
      break;

    case 'stepCompleted':
      vmStore.setPaused();
      editorStore.setHighlightedLine(message.line);
      break;

    case 'stopped':
      vmStore.reset();
      vmStore.setTimeoutWarning(null);
      editorStore.setHighlightedLine(null);
      break;

    case 'vmStateUpdate':
      set({
        debugState: message.state,
        locals: message.state.locals,
        stack: message.state.stack,
      });
      // Update highlighted line from VM state
      if (message.state.line !== null) {
        editorStore.setHighlightedLine(message.state.line);
      }
      break;

    case 'vmStateJson':
      // Full fiber/channel scheduler snapshot (fiber mode). Populate the rich
      // VM state so the Fibers/Channels inspectors render live concurrent state.
      vmStore.applyVmStateJson(message.state);
      break;

    case 'locals':
      set({ locals: message.locals });
      break;

    case 'stack':
      // Legacy message - convert strings to ValueJson format
      set({ stack: message.stack.map(s => ({ type: 'String', value: s } as ValueJson)) });
      break;

    case 'variableValue':
      // Could be used for watch expressions in the future
      console.log('Variable:', message.name, '=', message.value, `(${message.typeName})`);
      break;

    case 'fiberSpawned':
      vmStore.addFiber(message.fiber);
      break;

    case 'fiberStateChanged':
      vmStore.updateFiberState(message.fiberId, message.newState);
      break;

    case 'channelCreated':
      vmStore.addChannel(message.channel);
      break;

    case 'channelMessage':
      vmStore.recordChannelMessage(message.channelId, message.operation, message.value);
      break;

    case 'pong':
      // Keepalive response
      break;

    case 'timeoutWarning':
      vmStore.setTimeoutWarning(message.secondsRemaining);
      break;

    case 'ast':
      // AST response for getAst request
      compilerStore.setCompilationSuccess(message.ast);
      break;

    case 'breakpointsSet':
      // Breakpoints were successfully applied
      console.log('Breakpoints set:', message.breakpoints);
      break;

    default:
      console.warn('Unknown message type:', (message as ServerMessage).type);
  }
}

// Selectors
export const selectIsConnected = (state: WebSocketStoreState) =>
  state.connectionStatus === 'connected';

export const selectDebugState = (state: WebSocketStoreState) => state.debugState;

export const selectLocals = (state: WebSocketStoreState) => state.locals;

export const selectStack = (state: WebSocketStoreState) => state.stack;

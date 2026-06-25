import { create } from 'zustand';
import type { VmState, Fiber, Channel, FiberId, ChannelId, ExecutionStatus, FiberState } from '../types/vm';
import type { FiberInfo, ChannelInfo, VmStateJson } from '../types/protocol';

/** Event types for the event history */
export type FiberChannelEventType =
  | 'fiberSpawned'
  | 'fiberStateChanged'
  | 'channelCreated'
  | 'channelMessage';

/** Event history entry */
export interface FiberChannelEvent {
  timestamp: number;
  type: FiberChannelEventType;
  details: string;
}

const MAX_EVENT_HISTORY = 50;

export interface VmStoreState {
  /** Current execution status */
  executionStatus: ExecutionStatus;
  /** Complete VM state */
  vmState: VmState | null;
  /** Program output lines */
  output: string[];
  /** Exit code (when finished) */
  exitCode: number | null;
  /** Execution time in ms */
  executionTime: number | null;
  /** Error message (if any) */
  error: string | null;
  /** Currently selected fiber for inspection */
  selectedFiberId: FiberId | null;
  /** Currently selected channel for inspection */
  selectedChannelId: ChannelId | null;
  /** Timeout warning (seconds remaining, null if no warning) */
  timeoutWarning: number | null;
  /** Event history for fiber/channel events */
  eventHistory: FiberChannelEvent[];

  // Actions
  startExecution: () => void;
  setRunning: () => void;
  setPaused: (line?: number) => void;
  setFinished: (exitCode: number, executionTime: number) => void;
  setError: (error: string) => void;
  updateVmState: (state: VmState) => void;
  /** Apply a full fiber/channel scheduler snapshot from the backend (fiber mode) */
  applyVmStateJson: (state: VmStateJson) => void;
  appendOutput: (line: string) => void;
  clearOutput: () => void;
  selectFiber: (id: FiberId | null) => void;
  selectChannel: (id: ChannelId | null) => void;
  setTimeoutWarning: (seconds: number | null) => void;
  // Fiber/channel event actions
  addFiber: (fiber: FiberInfo) => void;
  updateFiberState: (fiberId: FiberId, newState: string) => void;
  addChannel: (channel: ChannelInfo) => void;
  recordChannelMessage: (channelId: ChannelId, operation: string, value: string) => void;
  clearEventHistory: () => void;
  reset: () => void;
}

/** Parse a fiber state string into the FiberState type */
function parseFiberState(stateStr: string): FiberState {
  if (stateStr === 'Ready') return { type: 'Ready' };
  if (stateStr === 'Running') return { type: 'Running' };
  if (stateStr === 'Yielded') return { type: 'Yielded' };
  if (stateStr === 'Finished') return { type: 'Finished' };
  if (stateStr === 'BlockedSelect') return { type: 'BlockedSelect' };

  const blockedReceiveMatch = stateStr.match(/BlockedReceive\((\d+)\)/);
  if (blockedReceiveMatch) {
    return { type: 'BlockedReceive', channelId: parseInt(blockedReceiveMatch[1], 10) };
  }

  const blockedSendMatch = stateStr.match(/BlockedSend\((\d+)\)/);
  if (blockedSendMatch) {
    return { type: 'BlockedSend', channelId: parseInt(blockedSendMatch[1], 10) };
  }

  const failedMatch = stateStr.match(/Failed\((.*)\)/);
  if (failedMatch) {
    return { type: 'Failed', error: failedMatch[1] };
  }

  // Default to Ready if we can't parse
  return { type: 'Ready' };
}

/** Add an event to history, keeping max size */
function addEvent(history: FiberChannelEvent[], event: FiberChannelEvent): FiberChannelEvent[] {
  const newHistory = [...history, event];
  if (newHistory.length > MAX_EVENT_HISTORY) {
    return newHistory.slice(-MAX_EVENT_HISTORY);
  }
  return newHistory;
}

const initialVmState: VmState = {
  fibers: {},
  channels: {},
  currentFiberId: null,
  readyQueue: [],
  output: [],
  exitCode: null,
};

export const useVmStore = create<VmStoreState>((set) => ({
  executionStatus: 'idle',
  vmState: null,
  output: [],
  exitCode: null,
  executionTime: null,
  error: null,
  selectedFiberId: null,
  selectedChannelId: null,
  timeoutWarning: null,
  eventHistory: [],

  startExecution: () => set({
    executionStatus: 'compiling',
    vmState: { ...initialVmState },
    output: [],
    exitCode: null,
    executionTime: null,
    error: null,
    eventHistory: [],
  }),

  setRunning: () => set({ executionStatus: 'running' }),

  setPaused: () => set({ executionStatus: 'paused' }),

  setFinished: (exitCode, executionTime) => set({
    executionStatus: 'finished',
    exitCode,
    executionTime,
  }),

  setError: (error) => set({
    executionStatus: 'error',
    error,
  }),

  updateVmState: (state) => set({ vmState: state }),

  applyVmStateJson: (wire: VmStateJson) => set(() => {
    // Convert the backend's id-sorted arrays into the store's id-keyed records,
    // carrying each fiber's live stack/locals/call-stack and each channel's
    // buffer/waiters so the inspectors render real concurrent state.
    const fibers: Record<FiberId, Fiber> = {};
    for (const f of wire.fibers) {
      fibers[f.id] = {
        id: f.id,
        state: f.state,
        ip: f.ip,
        stack: f.stack,
        locals: f.locals,
        callStack: f.callStack,
        fiberLocals: {},
        result: f.result,
      };
    }

    const channels: Record<ChannelId, Channel> = {};
    for (const c of wire.channels) {
      channels[c.id] = {
        id: c.id,
        buffer: c.buffer,
        capacity: c.capacity,
        receivers: c.receivers,
        senders: c.senders,
        closed: c.closed,
      };
    }

    return {
      vmState: {
        fibers,
        channels,
        currentFiberId: wire.currentFiberId,
        readyQueue: wire.readyQueue,
        output: wire.output,
        exitCode: wire.exitCode,
      },
    };
  }),

  appendOutput: (line) => set((state) => ({
    output: [...state.output, line],
  })),

  clearOutput: () => set({ output: [] }),

  selectFiber: (id) => set({ selectedFiberId: id }),

  selectChannel: (id) => set({ selectedChannelId: id }),

  setTimeoutWarning: (seconds) => set({ timeoutWarning: seconds }),

  addFiber: (fiber: FiberInfo) => set((state) => {
    const fibers = { ...state.vmState?.fibers ?? {} };
    fibers[fiber.id] = {
      id: fiber.id,
      state: parseFiberState(fiber.state),
      ip: fiber.ip,
      stack: [],
      locals: [],
      callStack: [],
      fiberLocals: {},
      result: null,
    };

    return {
      vmState: state.vmState
        ? { ...state.vmState, fibers }
        : { ...initialVmState, fibers },
      eventHistory: addEvent(state.eventHistory, {
        timestamp: Date.now(),
        type: 'fiberSpawned',
        details: `Fiber #${fiber.id} spawned at IP ${fiber.ip}`,
      }),
    };
  }),

  updateFiberState: (fiberId: FiberId, newState: string) => set((state) => {
    if (!state.vmState) return state;

    const fibers = { ...state.vmState.fibers };
    if (fibers[fiberId]) {
      fibers[fiberId] = {
        ...fibers[fiberId],
        state: parseFiberState(newState),
      };
    }

    return {
      vmState: { ...state.vmState, fibers },
      eventHistory: addEvent(state.eventHistory, {
        timestamp: Date.now(),
        type: 'fiberStateChanged',
        details: `Fiber #${fiberId} -> ${newState}`,
      }),
    };
  }),

  addChannel: (channel: ChannelInfo) => set((state) => {
    const channels = { ...state.vmState?.channels ?? {} };
    channels[channel.id] = {
      id: channel.id,
      buffer: [],
      capacity: channel.capacity,
      receivers: [],
      senders: [],
      closed: channel.closed,
    };

    return {
      vmState: state.vmState
        ? { ...state.vmState, channels }
        : { ...initialVmState, channels },
      eventHistory: addEvent(state.eventHistory, {
        timestamp: Date.now(),
        type: 'channelCreated',
        details: `Channel #${channel.id} created (capacity: ${channel.capacity})`,
      }),
    };
  }),

  recordChannelMessage: (channelId: ChannelId, operation: string, value: string) => set((state) => ({
    eventHistory: addEvent(state.eventHistory, {
      timestamp: Date.now(),
      type: 'channelMessage',
      details: `Channel #${channelId}: ${operation} ${value}`,
    }),
  })),

  clearEventHistory: () => set({ eventHistory: [] }),

  reset: () => set({
    executionStatus: 'idle',
    vmState: null,
    output: [],
    exitCode: null,
    executionTime: null,
    error: null,
    selectedFiberId: null,
    selectedChannelId: null,
    timeoutWarning: null,
    eventHistory: [],
  }),
}));

// Expose store on window for E2E testing
if (typeof window !== 'undefined') {
  (window as unknown as { __VM_STORE__: typeof useVmStore }).
    __VM_STORE__ = useVmStore;
}

// Selectors
export const selectFibers = (state: VmStoreState): Fiber[] => {
  if (!state.vmState) return [];
  return Object.values(state.vmState.fibers);
};

export const selectChannels = (state: VmStoreState): Channel[] => {
  if (!state.vmState) return [];
  return Object.values(state.vmState.channels);
};

export const selectCurrentFiber = (state: VmStoreState): Fiber | null => {
  if (!state.vmState || state.vmState.currentFiberId === null) return null;
  return state.vmState.fibers[state.vmState.currentFiberId] ?? null;
};

export const selectSelectedFiber = (state: VmStoreState): Fiber | null => {
  if (!state.vmState || state.selectedFiberId === null) return null;
  return state.vmState.fibers[state.selectedFiberId] ?? null;
};

export const selectSelectedChannel = (state: VmStoreState): Channel | null => {
  if (!state.vmState || state.selectedChannelId === null) return null;
  return state.vmState.channels[state.selectedChannelId] ?? null;
};

export const selectEventHistory = (state: VmStoreState): FiberChannelEvent[] => {
  return state.eventHistory;
};

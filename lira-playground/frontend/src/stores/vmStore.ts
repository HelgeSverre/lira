import { create } from 'zustand';
import type { VmState, Fiber, Channel, FiberId, ChannelId, ExecutionStatus } from '../types/vm';

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

  // Actions
  startExecution: () => void;
  setRunning: () => void;
  setPaused: (line?: number) => void;
  setFinished: (exitCode: number, executionTime: number) => void;
  setError: (error: string) => void;
  updateVmState: (state: VmState) => void;
  appendOutput: (line: string) => void;
  clearOutput: () => void;
  selectFiber: (id: FiberId | null) => void;
  selectChannel: (id: ChannelId | null) => void;
  reset: () => void;
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

  startExecution: () => set({
    executionStatus: 'compiling',
    vmState: { ...initialVmState },
    output: [],
    exitCode: null,
    executionTime: null,
    error: null,
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

  appendOutput: (line) => set((state) => ({
    output: [...state.output, line],
  })),

  clearOutput: () => set({ output: [] }),

  selectFiber: (id) => set({ selectedFiberId: id }),

  selectChannel: (id) => set({ selectedChannelId: id }),

  reset: () => set({
    executionStatus: 'idle',
    vmState: null,
    output: [],
    exitCode: null,
    executionTime: null,
    error: null,
    selectedFiberId: null,
    selectedChannelId: null,
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

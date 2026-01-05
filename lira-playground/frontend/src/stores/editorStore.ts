import { create } from 'zustand';
import { getSampleSource, getDefaultSampleId } from '../samples';

export interface EditorState {
  /** Current source code */
  sourceCode: string;
  /** Set of breakpoint line numbers (1-indexed) */
  breakpoints: Set<number>;
  /** Current cursor position */
  cursorPosition: { line: number; column: number };
  /** Currently highlighted line (during debugging) */
  highlightedLine: number | null;
  /** Whether the source has been modified since last compile */
  isDirty: boolean;

  // Actions
  setSourceCode: (code: string) => void;
  toggleBreakpoint: (line: number) => void;
  setBreakpoints: (lines: number[]) => void;
  clearBreakpoints: () => void;
  setCursorPosition: (line: number, column: number) => void;
  setHighlightedLine: (line: number | null) => void;
  markClean: () => void;
  loadSample: (id: string) => void;
}

export const useEditorStore = create<EditorState>((set) => ({
  sourceCode: getSampleSource(getDefaultSampleId()),
  breakpoints: new Set(),
  cursorPosition: { line: 1, column: 1 },
  highlightedLine: null,
  isDirty: false,

  // Note: Store is exposed on window for E2E testing (see bottom of file)

  setSourceCode: (code) => set({ sourceCode: code, isDirty: true }),

  toggleBreakpoint: (line) => set((state) => {
    const newBreakpoints = new Set(state.breakpoints);
    if (newBreakpoints.has(line)) {
      newBreakpoints.delete(line);
    } else {
      newBreakpoints.add(line);
    }
    return { breakpoints: newBreakpoints };
  }),

  setBreakpoints: (lines) => set({ breakpoints: new Set(lines) }),

  clearBreakpoints: () => set({ breakpoints: new Set() }),

  setCursorPosition: (line, column) => set({ cursorPosition: { line, column } }),

  setHighlightedLine: (line) => set({ highlightedLine: line }),

  markClean: () => set({ isDirty: false }),

  loadSample: (id) => set({
    sourceCode: getSampleSource(id),
    isDirty: false,
    breakpoints: new Set(),
    highlightedLine: null,
  }),
}));

// Expose store on window for E2E testing
if (typeof window !== 'undefined') {
  (window as unknown as { __EDITOR_STORE__: typeof useEditorStore }).
    __EDITOR_STORE__ = useEditorStore;
}

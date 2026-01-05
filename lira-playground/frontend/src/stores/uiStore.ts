import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type OutputTab = 'output' | 'debug' | 'vm' | 'fibers' | 'channels';

export interface UiState {
  /** Active tab in the output panel */
  activeOutputTab: OutputTab;
  /** Whether the AST panel is collapsed */
  astPanelCollapsed: boolean;
  /** Whether to show span info in AST */
  showSpanInfo: boolean;
  /** Whether to auto-scroll output */
  autoScrollOutput: boolean;
  /** Theme (for future use) */
  theme: 'dark' | 'light';
  /** Font size for editor */
  fontSize: number;
  /** Selected sample program */
  selectedSample: string;

  // Actions
  setActiveOutputTab: (tab: OutputTab) => void;
  toggleAstPanel: () => void;
  setShowSpanInfo: (show: boolean) => void;
  setAutoScrollOutput: (auto: boolean) => void;
  setTheme: (theme: 'dark' | 'light') => void;
  setFontSize: (size: number) => void;
  setSelectedSample: (sample: string) => void;
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      activeOutputTab: 'output',
      astPanelCollapsed: false,
      showSpanInfo: false,
      autoScrollOutput: true,
      theme: 'dark',
      fontSize: 14,
      selectedSample: 'hello-world',

      setActiveOutputTab: (tab) => set({ activeOutputTab: tab }),

      toggleAstPanel: () => set((state) => ({
        astPanelCollapsed: !state.astPanelCollapsed
      })),

      setShowSpanInfo: (show) => set({ showSpanInfo: show }),

      setAutoScrollOutput: (auto) => set({ autoScrollOutput: auto }),

      setTheme: (theme) => set({ theme }),

      setFontSize: (size) => set({ fontSize: size }),

      setSelectedSample: (sample) => set({ selectedSample: sample }),
    }),
    {
      name: 'lira-playground-ui',
      partialize: (state) => ({
        showSpanInfo: state.showSpanInfo,
        autoScrollOutput: state.autoScrollOutput,
        theme: state.theme,
        fontSize: state.fontSize,
      }),
    }
  )
);

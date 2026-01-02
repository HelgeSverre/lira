import { create } from 'zustand';
import type { Program } from '../types/ast';
import type { CompileError, BytecodeProgram } from '../types';

export type CompilationStatus = 'idle' | 'compiling' | 'success' | 'error';

export interface CompilerState {
  /** Current compilation status */
  status: CompilationStatus;
  /** Parsed AST (if compilation successful) */
  ast: Program | null;
  /** Compiled bytecode (if compilation successful) */
  bytecode: BytecodeProgram | null;
  /** Compilation errors */
  errors: CompileError[];
  /** Time taken to compile (ms) */
  compileTime: number | null;

  // Actions
  startCompilation: () => void;
  setCompilationSuccess: (ast: Program, bytecode?: BytecodeProgram) => void;
  setCompilationError: (errors: CompileError[]) => void;
  setAst: (ast: Program) => void;
  reset: () => void;
}

export const useCompilerStore = create<CompilerState>((set) => ({
  status: 'idle',
  ast: null,
  bytecode: null,
  errors: [],
  compileTime: null,

  startCompilation: () => set({
    status: 'compiling',
    errors: [],
    compileTime: null,
  }),

  setCompilationSuccess: (ast, bytecode) => set({
    status: 'success',
    ast,
    bytecode: bytecode ?? null,
    errors: [],
  }),

  setCompilationError: (errors) => set({
    status: 'error',
    errors,
    ast: null,
    bytecode: null,
  }),

  setAst: (ast) => set({ ast }),

  reset: () => set({
    status: 'idle',
    ast: null,
    bytecode: null,
    errors: [],
    compileTime: null,
  }),
}));

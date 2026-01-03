import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { useVmStore } from '../../stores/vmStore';
import { compile as compileCode, run as runCode } from '../../api/client';
import { useRef } from 'react';

export function RunButton() {
  const { sourceCode, markClean, breakpoints, setHighlightedLine } = useEditorStore();
  const { startCompilation, setCompilationSuccess, setCompilationError } = useCompilerStore();
  const {
    executionStatus,
    startExecution,
    setRunning,
    setFinished,
    setPaused,
    setError,
    appendOutput,
    reset,
  } = useVmStore();

  const abortControllerRef = useRef<AbortController | null>(null);

  const isRunning = executionStatus === 'running';
  const isCompiling = executionStatus === 'compiling';
  const isPaused = executionStatus === 'paused';

  const handleRun = async () => {
    // Reset previous state
    reset();
    startExecution();
    startCompilation();

    try {
      abortControllerRef.current = new AbortController();

      // Step 1: Compile first to get AST and check for errors
      const compileResult = await compileCode(sourceCode);

      if (!compileResult.success || compileResult.errors.length > 0) {
        setCompilationError(compileResult.errors);
        setError(compileResult.errors[0]?.message || 'Compilation failed');
        return;
      }

      // Update AST in compiler store
      if (compileResult.ast) {
        setCompilationSuccess(compileResult.ast);
      }

      // Step 2: Now run the code (pass breakpoints from editor)
      setRunning();
      const breakpointLines = Array.from(breakpoints);
      const result = await runCode(sourceCode, breakpointLines);

      // Output lines (whether finished or paused at breakpoint)
      for (const line of result.output) {
        appendOutput(line);
      }

      if (result.breakpoint) {
        // Execution paused at a breakpoint
        setPaused();
        setHighlightedLine(result.breakpoint.line);
      } else if (result.success) {
        // Execution completed successfully
        markClean();
        setHighlightedLine(null);
        setFinished(result.exitCode ?? 0, result.executionTimeMs);
      } else {
        // Actual error occurred
        setHighlightedLine(null);
        setCompilationError(result.errors);
        setError(result.errors[0]?.message || 'Execution failed');
      }
    } catch (error) {
      console.error('Run failed:', error);
      if (error instanceof Error && error.name !== 'AbortError') {
        setError(error.message);
      } else {
        setError('An unexpected error occurred');
      }
    } finally {
      abortControllerRef.current = null;
    }
  };

  const handleStop = () => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    reset();
  };

  if (isRunning) {
    return (
      <button
        className="action-button secondary"
        onClick={handleStop}
        title="Stop (Escape)"
      >
        <StopIcon />
        Stop
      </button>
    );
  }

  if (isPaused) {
    return (
      <button
        className="action-button warning"
        onClick={handleRun}
        title="Continue (F5)"
      >
        <PauseIcon />
        Paused
      </button>
    );
  }

  return (
    <button
      className="action-button primary"
      onClick={handleRun}
      disabled={isCompiling}
      title="Run (Ctrl+Shift+Enter)"
    >
      {isCompiling ? (
        <>
          <span className="spinner" />
          Compiling...
        </>
      ) : (
        <>
          <PlayIcon />
          Run
        </>
      )}
    </button>
  );
}

function PauseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
      <rect x="4" y="3" width="3" height="10" rx="0.5" />
      <rect x="9" y="3" width="3" height="10" rx="0.5" />
    </svg>
  );
}

function PlayIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
      <path d="M4 2.5v11l9-5.5L4 2.5z" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
      <rect x="3" y="3" width="10" height="10" rx="1" />
    </svg>
  );
}

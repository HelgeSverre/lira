import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { useVmStore } from '../../stores/vmStore';
import { useWebSocketStore } from '../../stores/websocketStore';
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
    setError,
    appendOutput,
    reset,
  } = useVmStore();
  const { startDebug, stop: wsStop, connectionStatus } = useWebSocketStore();

  const abortControllerRef = useRef<AbortController | null>(null);

  const isRunning = executionStatus === 'running';
  const isCompiling = executionStatus === 'compiling';
  const isPaused = executionStatus === 'paused';
  const hasBreakpoints = breakpoints.size > 0;

  const handleRun = async () => {
    // Reset previous state
    reset();
    startExecution();
    startCompilation();

    const breakpointLines = Array.from(breakpoints);

    // Use WebSocket for debug mode (when breakpoints are set)
    if (hasBreakpoints) {
      // WebSocket-based debug execution
      startDebug(sourceCode, breakpointLines);
      return;
    }

    // HTTP-based simple execution (no breakpoints)
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

      // Step 2: Now run the code
      setRunning();
      const result = await runCode(sourceCode, []);

      // Output lines
      for (const line of result.output) {
        appendOutput(line);
      }

      if (result.success) {
        // Execution completed successfully
        markClean();
        setHighlightedLine(null);
        setFinished(result.exitCode ?? 0, result.executionTimeMs);
      } else {
        // Error occurred
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
    // Stop WebSocket execution if connected
    if (connectionStatus === 'connected') {
      wsStop();
    }
    // Also abort any HTTP request
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    reset();
    setHighlightedLine(null);
  };

  if (isRunning) {
    return (
      <button
        className="action-button secondary"
        onClick={handleStop}
        title="Stop (Escape)"
        data-testid="run-button"
        data-state="running"
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
        data-testid="run-button"
        data-state="paused"
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
      data-testid="run-button"
      data-state={isCompiling ? 'compiling' : 'idle'}
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

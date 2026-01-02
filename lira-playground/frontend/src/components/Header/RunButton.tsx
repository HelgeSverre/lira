import { useEditorStore } from '../../stores/editorStore';
import { useCompilerStore } from '../../stores/compilerStore';
import { useVmStore } from '../../stores/vmStore';
import { mockCompile } from '../../mocks/mockCompiler';
import { MockVm } from '../../mocks/mockVm';
import { useRef } from 'react';

export function RunButton() {
  const { sourceCode, markClean } = useEditorStore();
  const { startCompilation, setCompilationSuccess, setCompilationError } = useCompilerStore();
  const {
    executionStatus,
    startExecution,
    setRunning,
    setFinished,
    setError,
    appendOutput,
    updateVmState,
  } = useVmStore();

  const vmRef = useRef<MockVm | null>(null);

  const isRunning = executionStatus === 'running';
  const isCompiling = executionStatus === 'compiling';

  const handleRun = async () => {
    // Start execution (shows compiling state)
    startExecution();
    startCompilation();

    // Compile first
    await new Promise(resolve => setTimeout(resolve, 100));
    const result = mockCompile(sourceCode);

    if (result.errors.length > 0) {
      setCompilationError(result.errors);
      setError('Compilation failed');
      return;
    }

    setCompilationSuccess(result.ast);
    markClean();

    // Create and run VM
    setRunning();
    const startTime = Date.now();

    vmRef.current = new MockVm({
      onOutput: (line) => appendOutput(line),
      onStateUpdate: (state) => updateVmState(state),
      onFinished: (exitCode) => {
        const duration = Date.now() - startTime;
        setFinished(exitCode, duration);
        vmRef.current = null;
      },
      onError: (error) => {
        setError(error);
        vmRef.current = null;
      },
    });

    vmRef.current.run();
  };

  const handleStop = () => {
    if (vmRef.current) {
      vmRef.current.stop();
      vmRef.current = null;
    }
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

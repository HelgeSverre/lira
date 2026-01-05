import { useEditorStore } from '../../stores/editorStore';
import { useVmStore } from '../../stores/vmStore';
import { useWebSocketStore } from '../../stores/websocketStore';
import './DebugControls.css';

export function DebugControls() {
  const { setHighlightedLine } = useEditorStore();
  const { executionStatus, reset } = useVmStore();
  const {
    continueExecution,
    stepInto,
    stepOver,
    stepOut,
    pause,
    stop,
    connectionStatus,
  } = useWebSocketStore();

  const isPaused = executionStatus === 'paused';
  const isRunning = executionStatus === 'running';
  const canStep = isPaused;
  const isConnected = connectionStatus === 'connected';

  const handleContinue = () => {
    if (isPaused && isConnected) {
      continueExecution();
    }
  };

  const handleStepInto = () => {
    if (canStep && isConnected) {
      stepInto();
    }
  };

  const handleStepOver = () => {
    if (canStep && isConnected) {
      stepOver();
    }
  };

  const handleStepOut = () => {
    if (canStep && isConnected) {
      stepOut();
    }
  };

  const handlePause = () => {
    if (isRunning && isConnected) {
      pause();
    }
  };

  const handleReset = () => {
    if (isConnected) {
      stop();
    } else {
      reset();
      setHighlightedLine(null);
    }
  };

  return (
    <div className="debug-controls">
      {isRunning ? (
        <button
          className="icon-button"
          onClick={handlePause}
          title="Pause execution (F6)"
        >
          <PauseIcon />
        </button>
      ) : (
        <button
          className="icon-button"
          onClick={handleContinue}
          disabled={!isPaused}
          title="Continue to next breakpoint (F5)"
        >
          <PlayIcon />
        </button>
      )}

      <button
        className="icon-button"
        onClick={handleStepInto}
        disabled={!canStep}
        title="Step to next line (F11)"
      >
        <StepIntoIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleStepOver}
        disabled={!canStep}
        title="Step over function call (F10)"
      >
        <StepOverIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleStepOut}
        disabled={!canStep}
        title="Step out of current function (Shift+F11)"
      >
        <StepOutIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleReset}
        title="Stop debugging and reset (Ctrl+Shift+F5)"
      >
        <ResetIcon />
      </button>
    </div>
  );
}

function PlayIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M4 2.5v11l9-5.5L4 2.5z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <rect x="4" y="3" width="3" height="10" rx="0.5" />
      <rect x="9" y="3" width="3" height="10" rx="0.5" />
    </svg>
  );
}

function StepIntoIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 2v8M4 6l4 4 4-4" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="8" cy="13" r="1.5" />
    </svg>
  );
}

function StepOverIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M2 8h8M6 4l4 4-4 4" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="13" cy="8" r="1.5" />
    </svg>
  );
}

function StepOutIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 14V6M4 10l4-4 4 4" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
      <circle cx="8" cy="3" r="1.5" />
    </svg>
  );
}

function ResetIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M3 8a5 5 0 019.584-2M13 8a5 5 0 01-9.584 2" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" />
      <path d="M13 3v3h-3M3 13v-3h3" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

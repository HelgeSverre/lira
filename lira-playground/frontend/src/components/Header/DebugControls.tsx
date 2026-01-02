import { useVmStore } from '../../stores/vmStore';
import './DebugControls.css';

export function DebugControls() {
  const { executionStatus } = useVmStore();

  const isPaused = executionStatus === 'paused';
  const isRunning = executionStatus === 'running';
  const canStep = isPaused;

  // These will be connected to actual functionality later
  const handleContinue = () => {
    console.log('Continue');
  };

  const handlePause = () => {
    console.log('Pause');
  };

  const handleStep = () => {
    console.log('Step');
  };

  const handleStepOver = () => {
    console.log('Step Over');
  };

  const handleStepOut = () => {
    console.log('Step Out');
  };

  const handleReset = () => {
    console.log('Reset');
  };

  return (
    <div className="debug-controls">
      {isRunning ? (
        <button
          className="icon-button"
          onClick={handlePause}
          title="Pause (F6)"
        >
          <PauseIcon />
        </button>
      ) : (
        <button
          className="icon-button"
          onClick={handleContinue}
          disabled={!isPaused}
          title="Continue (F5)"
        >
          <PlayIcon />
        </button>
      )}

      <button
        className="icon-button"
        onClick={handleStep}
        disabled={!canStep}
        title="Step Into (F11)"
      >
        <StepIntoIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleStepOver}
        disabled={!canStep}
        title="Step Over (F10)"
      >
        <StepOverIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleStepOut}
        disabled={!canStep}
        title="Step Out (Shift+F11)"
      >
        <StepOutIcon />
      </button>

      <button
        className="icon-button"
        onClick={handleReset}
        title="Reset (Ctrl+Shift+F5)"
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

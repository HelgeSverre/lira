import { step } from '../../api/client';
import type { StepType } from '../../api/client';
import { useEditorStore } from '../../stores/editorStore';
import { useVmStore } from '../../stores/vmStore';
import './DebugControls.css';

export function DebugControls() {
  const { sourceCode, breakpoints, highlightedLine, setHighlightedLine } = useEditorStore();
  const { executionStatus, setPaused, setFinished, appendOutput, reset, clearOutput } = useVmStore();

  const isPaused = executionStatus === 'paused';
  const isRunning = executionStatus === 'running';
  const canStep = isPaused;
  const currentLine = highlightedLine ?? 1;

  const handleStep = async (stepType: StepType) => {
    if (!isPaused && stepType !== 'continue') return;

    try {
      // Clear previous output when stepping (since we re-run from start)
      clearOutput();

      const result = await step({
        source: sourceCode,
        currentLine,
        stepType,
        breakpoints: Array.from(breakpoints),
      });

      // Handle output
      for (const line of result.output) {
        appendOutput(line);
      }

      if (result.breakpoint) {
        setPaused();
        setHighlightedLine(result.breakpoint.line);
      } else if (result.success) {
        setHighlightedLine(null);
        setFinished(result.exitCode ?? 0, result.executionTimeMs);
      }
    } catch (error) {
      console.error('Step failed:', error);
    }
  };

  const handleContinue = () => handleStep('continue');
  const handleStepInto = () => handleStep('into');
  const handleStepOver = () => handleStep('over');
  const handleStepOut = () => handleStep('out');

  const handlePause = () => {
    // Pause not implemented yet (would need WebSocket for async interruption)
    console.log('Pause not implemented');
  };

  const handleReset = () => {
    reset();
    setHighlightedLine(null);
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

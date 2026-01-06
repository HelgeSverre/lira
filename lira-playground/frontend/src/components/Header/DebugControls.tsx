import * as Tooltip from '@radix-ui/react-tooltip';
import {
  Play,
  Pause,
  ArrowDownToLine,
  ArrowRightToLine,
  ArrowUpFromLine,
  Square,
  Code,
  MoveRight,
  type LucideIcon,
} from 'lucide-react';
import { useEditorStore } from '../../stores/editorStore';
import { useVmStore } from '../../stores/vmStore';
import { useWebSocketStore } from '../../stores/websocketStore';
import './DebugControls.css';

interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon: LucideIcon;
  label: string;
  shortcut?: string;
}

function IconButton({ icon: Icon, label, shortcut, ...props }: IconButtonProps) {
  // Convert label to kebab-case for test id (e.g., "Step Into" -> "step-into")
  const testId = `debug-${label.toLowerCase().replace(/\s+/g, '-')}`;
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button className="icon-button" aria-label={label} data-testid={testId} {...props}>
          <Icon size={16} />
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="tooltip-content" sideOffset={5}>
          {label}
          {shortcut && <span className="tooltip-shortcut">{shortcut}</span>}
          <Tooltip.Arrow className="tooltip-arrow" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

export function DebugControls() {
  const { setHighlightedLine } = useEditorStore();
  const { executionStatus, reset } = useVmStore();
  const {
    continueExecution,
    stepInto,
    stepOver,
    stepOut,
    stepLine,
    stepInstruction,
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

  const handleStepLine = () => {
    if (canStep && isConnected) {
      stepLine();
    }
  };

  const handleStepInstruction = () => {
    if (canStep && isConnected) {
      stepInstruction();
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
    <Tooltip.Provider delayDuration={200}>
      <div className="debug-controls" data-testid="debug-controls">
        {isRunning ? (
          <IconButton
            icon={Pause}
            label="Pause"
            shortcut="F6"
            onClick={handlePause}
          />
        ) : (
          <IconButton
            icon={Play}
            label="Continue"
            shortcut="F5"
            onClick={handleContinue}
            disabled={!isPaused}
          />
        )}

        <IconButton
          icon={ArrowDownToLine}
          label="Step Into"
          shortcut="F11"
          onClick={handleStepInto}
          disabled={!canStep}
        />

        <IconButton
          icon={ArrowRightToLine}
          label="Step Over"
          shortcut="F10"
          onClick={handleStepOver}
          disabled={!canStep}
        />

        <IconButton
          icon={ArrowUpFromLine}
          label="Step Out"
          shortcut="Shift+F11"
          onClick={handleStepOut}
          disabled={!canStep}
        />

        <div className="debug-controls-separator" />

        <IconButton
          icon={MoveRight}
          label="Step Line"
          shortcut="Ctrl+Shift+L"
          onClick={handleStepLine}
          disabled={!canStep}
        />

        <IconButton
          icon={Code}
          label="Step Instruction"
          shortcut="Ctrl+Shift+I"
          onClick={handleStepInstruction}
          disabled={!canStep}
        />

        <div className="debug-controls-separator" />

        <IconButton
          icon={Square}
          label="Stop"
          shortcut="Ctrl+Shift+F5"
          onClick={handleReset}
        />
      </div>
    </Tooltip.Provider>
  );
}

import { Workflow } from 'lucide-react';
import { useUiStore } from '../../stores/uiStore';
import './FiberModeToggle.css';

/**
 * Toggles concurrent fiber mode. When on, Run/Debug route through the
 * scheduler-aware backend path and the Fibers/Channels tabs show live state.
 */
export function FiberModeToggle() {
  const { fiberMode, toggleFiberMode } = useUiStore();

  return (
    <button
      className="fiber-mode-toggle"
      role="switch"
      aria-checked={fiberMode}
      aria-label="Concurrent fiber mode"
      data-active={fiberMode}
      data-testid="fiber-mode-toggle"
      onClick={toggleFiberMode}
      title={
        fiberMode
          ? 'Fiber mode ON — Run drives the scheduler and shows live fibers/channels'
          : 'Fiber mode OFF — enable to debug concurrent (spawn/chan/select) programs'
      }
    >
      <Workflow size={14} />
      <span>Fibers</span>
    </button>
  );
}

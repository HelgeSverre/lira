import { useEffect, useRef } from 'react';
import { useVmStore } from '../../stores/vmStore';
import { useUiStore } from '../../stores/uiStore';
import './OutputConsole.css';

export function OutputConsole() {
  const { output, executionStatus, error, exitCode, executionTime } = useVmStore();
  const { autoScrollOutput } = useUiStore();
  const containerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScrollOutput && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [output, autoScrollOutput]);

  const isIdle = executionStatus === 'idle';
  const hasOutput = output.length > 0 || error || exitCode !== null;

  return (
    <div className="console" ref={containerRef} data-testid="output-console" data-status={executionStatus}>
      {isIdle && !hasOutput && (
        <div className="console-placeholder" data-testid="console-placeholder">
          Click "Run" to see output
        </div>
      )}

      {output.map((line, i) => (
        <div key={i} className="console-line" data-testid="console-line">
          {line}
        </div>
      ))}

      {error && (
        <div className="console-line error" data-testid="console-error">
          Error: {error}
        </div>
      )}

      {executionStatus === 'finished' && (
        <div className="console-line finished" data-testid="console-finished">
          Process exited with code {exitCode} ({executionTime}ms)
        </div>
      )}

      {executionStatus === 'running' && (
        <div className="console-line running" data-testid="console-running">
          Running...
        </div>
      )}

      {executionStatus === 'paused' && (
        <div className="console-line paused" data-testid="console-paused">
          Paused at breakpoint
        </div>
      )}
    </div>
  );
}

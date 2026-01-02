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
    <div className="console" ref={containerRef}>
      {isIdle && !hasOutput && (
        <div className="console-placeholder">
          Click "Run" to see output
        </div>
      )}

      {output.map((line, i) => (
        <div key={i} className="console-line">
          {line}
        </div>
      ))}

      {error && (
        <div className="console-line error">
          Error: {error}
        </div>
      )}

      {executionStatus === 'finished' && (
        <div className="console-line finished">
          Process exited with code {exitCode} ({executionTime}ms)
        </div>
      )}

      {executionStatus === 'running' && (
        <div className="console-line running">
          Running...
        </div>
      )}

      {executionStatus === 'paused' && (
        <div className="console-line paused">
          Paused at breakpoint
        </div>
      )}
    </div>
  );
}

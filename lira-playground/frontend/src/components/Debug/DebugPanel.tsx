/**
 * Debug Panel
 *
 * Displays debug information during stepping: locals, stack, call stack.
 */

import { useWebSocketStore, selectLocals, selectStack, selectDebugState } from '../../stores/websocketStore';
import { useVmStore } from '../../stores/vmStore';
import { ExpandableValue } from './ExpandableValue';
import './DebugPanel.css';

export function DebugPanel() {
  const locals = useWebSocketStore(selectLocals);
  const stack = useWebSocketStore(selectStack);
  const debugState = useWebSocketStore(selectDebugState);
  const { executionStatus } = useVmStore();

  const isPaused = executionStatus === 'paused';
  const isDebugging = isPaused || executionStatus === 'running';

  if (!isDebugging) {
    return (
      <div className="debug-panel" data-testid="debug-panel" data-state="idle">
        <div className="debug-panel-empty" data-testid="debug-empty">
          <p>Not debugging</p>
          <p className="hint">Set breakpoints and run to start debugging</p>
        </div>
      </div>
    );
  }

  return (
    <div className="debug-panel" data-testid="debug-panel" data-state={executionStatus}>
      {/* Execution State */}
      {debugState && (
        <section className="debug-section" data-testid="debug-execution">
          <h3>Execution</h3>
          <div className="debug-info">
            <span className="label">State:</span>
            <span className={`value state-${executionStatus}`}>{debugState.executionState}</span>
          </div>
          {debugState.line !== null && (
            <div className="debug-info">
              <span className="label">Line:</span>
              <span className="value">{debugState.line}:{debugState.column ?? 0}</span>
            </div>
          )}
          <div className="debug-info">
            <span className="label">IP:</span>
            <span className="value mono">{debugState.ip}</span>
          </div>
        </section>
      )}

      {/* Local Variables */}
      <section className="debug-section" data-testid="debug-locals">
        <h3>Locals</h3>
        {locals.length === 0 ? (
          <div className="debug-empty">No local variables</div>
        ) : (
          <div className="debug-list">
            {locals.map((local, index) => (
              <div key={index} className="debug-item local-item" data-testid="local-var" data-name={local.name}>
                <span className="var-name">{local.name}</span>
                <span className="var-type">{local.typeName}</span>
                <span className="var-value">
                  <ExpandableValue value={local.value} />
                </span>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Stack */}
      <section className="debug-section" data-testid="debug-stack">
        <h3>Stack</h3>
        {stack.length === 0 ? (
          <div className="debug-empty">Stack empty</div>
        ) : (
          <div className="debug-list stack-list">
            {stack.map((value, index) => (
              <div key={index} className="debug-item stack-item" data-testid="stack-value">
                <span className="stack-index">{stack.length - 1 - index}</span>
                <span className="stack-value">
                  <ExpandableValue value={value} />
                </span>
              </div>
            )).reverse()}
          </div>
        )}
      </section>

      {/* Call Stack */}
      {debugState && debugState.callStack.length > 0 && (
        <section className="debug-section" data-testid="debug-callstack">
          <h3>Call Stack</h3>
          <div className="debug-list">
            {debugState.callStack.map((frame, index) => (
              <div key={index} className="debug-item call-frame" data-testid="call-frame">
                <span className="frame-index">{index}</span>
                <span className="frame-name">{frame}</span>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

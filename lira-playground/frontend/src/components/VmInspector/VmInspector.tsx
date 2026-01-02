import { useVmStore, selectCurrentFiber } from '../../stores/vmStore';
import { StackView } from './StackView';
import { LocalsView } from './LocalsView';
import './VmInspector.css';

export function VmInspector() {
  const vmStore = useVmStore();
  const { vmState, executionStatus } = vmStore;
  const currentFiber = selectCurrentFiber(vmStore);

  const isIdle = executionStatus === 'idle';

  if (isIdle || !vmState) {
    return (
      <div className="vm-inspector">
        <div className="vm-placeholder">
          Run code to inspect VM state
        </div>
      </div>
    );
  }

  return (
    <div className="vm-inspector">
      <div className="vm-section">
        <div className="vm-section-header">
          <span>Current Fiber</span>
          {currentFiber && (
            <span className="vm-info">#{currentFiber.id}</span>
          )}
        </div>
        {currentFiber ? (
          <div className="vm-section-content">
            <div className="vm-stat">
              <span className="vm-stat-label">State:</span>
              <span className={`state-badge ${currentFiber.state.type.toLowerCase()}`}>
                {currentFiber.state.type}
              </span>
            </div>
            <div className="vm-stat">
              <span className="vm-stat-label">IP:</span>
              <span className="vm-stat-value">{currentFiber.ip}</span>
            </div>
          </div>
        ) : (
          <div className="vm-placeholder-small">No active fiber</div>
        )}
      </div>

      <div className="vm-section">
        <div className="vm-section-header">Stack</div>
        <StackView stack={currentFiber?.stack ?? []} />
      </div>

      <div className="vm-section">
        <div className="vm-section-header">Locals</div>
        <LocalsView locals={currentFiber?.locals ?? []} />
      </div>

      <div className="vm-section">
        <div className="vm-section-header">Call Stack</div>
        <div className="vm-section-content">
          {currentFiber && currentFiber.callStack.length > 0 ? (
            currentFiber.callStack.map((frame, i) => (
              <div key={i} className="call-frame">
                <span className="frame-index">{i}</span>
                <span className="frame-name">
                  {frame.functionName || `<frame@${frame.returnAddr}>`}
                </span>
              </div>
            ))
          ) : (
            <div className="vm-placeholder-small">Empty call stack</div>
          )}
        </div>
      </div>
    </div>
  );
}

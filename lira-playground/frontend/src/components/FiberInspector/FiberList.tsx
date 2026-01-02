import type { Fiber, FiberId } from '../../types/vm';
import { getFiberStateClass } from '../../types/vm';
import { useVmStore } from '../../stores/vmStore';
import './FiberList.css';

interface FiberListProps {
  fibers: Fiber[];
  currentFiberId: FiberId | null;
}

export function FiberList({ fibers, currentFiberId }: FiberListProps) {
  const { selectedFiberId, selectFiber } = useVmStore();

  if (fibers.length === 0) {
    return <div className="fiber-empty">No fibers</div>;
  }

  return (
    <div className="fiber-list">
      {fibers.map((fiber) => (
        <div
          key={fiber.id}
          className={`fiber-item ${fiber.id === selectedFiberId ? 'selected' : ''} ${fiber.id === currentFiberId ? 'current' : ''}`}
          onClick={() => selectFiber(fiber.id)}
        >
          <div className="fiber-header">
            <span className="fiber-id">Fiber #{fiber.id}</span>
            {fiber.id === currentFiberId && (
              <span className="fiber-current-badge">current</span>
            )}
            <span className={`state-badge ${getFiberStateClass(fiber.state)}`}>
              {fiber.state.type}
            </span>
          </div>
          <div className="fiber-details">
            <span className="fiber-stat">
              IP: {fiber.ip}
            </span>
            <span className="fiber-stat">
              Stack: {fiber.stack.length}
            </span>
            <span className="fiber-stat">
              Locals: {fiber.locals.length}
            </span>
          </div>
          {fiber.state.type === 'BlockedReceive' && (
            <div className="fiber-blocked-info">
              Waiting to receive from channel #{fiber.state.channelId}
            </div>
          )}
          {fiber.state.type === 'BlockedSend' && (
            <div className="fiber-blocked-info">
              Waiting to send to channel #{fiber.state.channelId}
            </div>
          )}
          {fiber.state.type === 'Failed' && (
            <div className="fiber-error">
              Error: {fiber.state.error}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

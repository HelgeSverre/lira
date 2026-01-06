import { useVmStore, selectEventHistory } from '../../stores/vmStore';
import './EventTimeline.css';

export function EventTimeline() {
  const eventHistory = useVmStore(selectEventHistory);

  if (eventHistory.length === 0) {
    return (
      <div className="event-timeline-empty" data-testid="event-timeline-empty">
        No events recorded yet. Run a program with fibers or channels to see events.
      </div>
    );
  }

  return (
    <div className="event-timeline" data-testid="event-timeline">
      <div className="event-timeline-header">
        <span className="event-count">{eventHistory.length} events</span>
      </div>
      <div className="event-timeline-list">
        {eventHistory.map((event, index) => (
          <div
            key={index}
            className={`event-item event-${event.type}`}
            data-testid="event-item"
          >
            <span className="event-time">
              {formatTimestamp(event.timestamp)}
            </span>
            <span className={`event-badge event-badge-${event.type}`}>
              {formatEventType(event.type)}
            </span>
            <span className="event-details">{event.details}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function formatTimestamp(timestamp: number): string {
  const date = new Date(timestamp);
  const hours = date.getHours().toString().padStart(2, '0');
  const minutes = date.getMinutes().toString().padStart(2, '0');
  const seconds = date.getSeconds().toString().padStart(2, '0');
  const ms = date.getMilliseconds().toString().padStart(3, '0');
  return `${hours}:${minutes}:${seconds}.${ms}`;
}

function formatEventType(type: string): string {
  switch (type) {
    case 'fiberSpawned':
      return 'SPAWN';
    case 'fiberStateChanged':
      return 'STATE';
    case 'channelCreated':
      return 'CHAN';
    case 'channelMessage':
      return 'MSG';
    default:
      return type.toUpperCase();
  }
}

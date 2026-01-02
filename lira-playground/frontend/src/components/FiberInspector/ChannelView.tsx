import type { Channel } from '../../types/vm';
import { ValueDisplay } from '../VmInspector/ValueDisplay';
import './ChannelView.css';

interface ChannelViewProps {
  channels: Channel[];
}

export function ChannelView({ channels }: ChannelViewProps) {
  if (channels.length === 0) {
    return <div className="channel-empty">No channels</div>;
  }

  return (
    <div className="channel-list">
      {channels.map((channel) => (
        <div key={channel.id} className={`channel-item ${channel.closed ? 'closed' : ''}`}>
          <div className="channel-header">
            <span className="channel-id">Channel #{channel.id}</span>
            {channel.closed && (
              <span className="channel-closed-badge">closed</span>
            )}
            <span className="channel-capacity">
              {channel.buffer.length}/{channel.capacity || '∞'}
            </span>
          </div>

          <div className="channel-details">
            <div className="channel-stat">
              <span className="stat-label">Waiting to receive:</span>
              <span className="stat-value">
                {channel.receivers.length > 0
                  ? channel.receivers.map(id => `#${id}`).join(', ')
                  : 'none'}
              </span>
            </div>
            <div className="channel-stat">
              <span className="stat-label">Waiting to send:</span>
              <span className="stat-value">
                {channel.senders.length > 0
                  ? channel.senders.map(s => `#${s.fiberId}`).join(', ')
                  : 'none'}
              </span>
            </div>
          </div>

          {channel.buffer.length > 0 && (
            <div className="channel-buffer">
              <div className="buffer-label">Buffer:</div>
              <div className="buffer-values">
                {channel.buffer.map((value, i) => (
                  <div key={i} className="buffer-value">
                    <span className="buffer-index">{i}</span>
                    <ValueDisplay value={value} showType={false} />
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

import { useVmStore, selectFibers, selectChannels } from '../../stores/vmStore';
import { FiberList } from './FiberList';
import { ChannelView } from './ChannelView';
import './FiberInspector.css';

interface FiberInspectorProps {
  mode: 'fibers' | 'channels';
}

export function FiberInspector({ mode }: FiberInspectorProps) {
  const vmStore = useVmStore();
  const { vmState, executionStatus } = vmStore;
  const fibers = selectFibers(vmStore);
  const channels = selectChannels(vmStore);

  const isIdle = executionStatus === 'idle';

  if (isIdle || !vmState) {
    return (
      <div className="fiber-inspector">
        <div className="fiber-placeholder">
          Run code to see {mode === 'fibers' ? 'fiber' : 'channel'} state
        </div>
      </div>
    );
  }

  if (mode === 'fibers') {
    return (
      <div className="fiber-inspector">
        <FiberList fibers={fibers} currentFiberId={vmState.currentFiberId} />
      </div>
    );
  }

  return (
    <div className="fiber-inspector">
      <ChannelView channels={channels} />
    </div>
  );
}

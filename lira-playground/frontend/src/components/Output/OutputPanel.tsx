import { useUiStore, OutputTab } from '../../stores/uiStore';
import { OutputConsole } from './OutputConsole';
import { VmInspector } from '../VmInspector/VmInspector';
import { FiberInspector } from '../FiberInspector/FiberInspector';
import { DebugPanel } from '../Debug/DebugPanel';
import './OutputPanel.css';

const TABS: { id: OutputTab; label: string }[] = [
  { id: 'output', label: 'Output' },
  { id: 'debug', label: 'Debug' },
  { id: 'vm', label: 'VM' },
  { id: 'fibers', label: 'Fibers' },
  { id: 'channels', label: 'Channels' },
];

export function OutputPanel() {
  const { activeOutputTab, setActiveOutputTab } = useUiStore();

  return (
    <div className="panel output-panel" data-testid="output-panel">
      <div className="tab-bar" data-testid="output-tabs">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            className={`tab ${activeOutputTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveOutputTab(tab.id)}
            data-testid={`tab-${tab.id}`}
            data-active={activeOutputTab === tab.id}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="panel-content output-content" data-testid="output-content">
        {activeOutputTab === 'output' && <OutputConsole />}
        {activeOutputTab === 'debug' && <DebugPanel />}
        {activeOutputTab === 'vm' && <VmInspector />}
        {activeOutputTab === 'fibers' && <FiberInspector mode="fibers" />}
        {activeOutputTab === 'channels' && <FiberInspector mode="channels" />}
      </div>
    </div>
  );
}

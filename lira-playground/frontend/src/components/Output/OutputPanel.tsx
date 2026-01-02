import { useUiStore, OutputTab } from '../../stores/uiStore';
import { OutputConsole } from './OutputConsole';
import { VmInspector } from '../VmInspector/VmInspector';
import { FiberInspector } from '../FiberInspector/FiberInspector';
import './OutputPanel.css';

const TABS: { id: OutputTab; label: string }[] = [
  { id: 'output', label: 'Output' },
  { id: 'vm', label: 'VM' },
  { id: 'fibers', label: 'Fibers' },
  { id: 'channels', label: 'Channels' },
];

export function OutputPanel() {
  const { activeOutputTab, setActiveOutputTab } = useUiStore();

  return (
    <div className="panel output-panel">
      <div className="tab-bar">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            className={`tab ${activeOutputTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveOutputTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="panel-content output-content">
        {activeOutputTab === 'output' && <OutputConsole />}
        {activeOutputTab === 'vm' && <VmInspector />}
        {activeOutputTab === 'fibers' && <FiberInspector mode="fibers" />}
        {activeOutputTab === 'channels' && <FiberInspector mode="channels" />}
      </div>
    </div>
  );
}

import { useEditorStore } from '../../stores/editorStore';
import { useUiStore } from '../../stores/uiStore';
import { SAMPLE_PROGRAM_LABELS, SampleProgramName } from '../../mocks/samplePrograms';
import './SampleSelector.css';

export function SampleSelector() {
  const { loadSample } = useEditorStore();
  const { selectedSample, setSelectedSample } = useUiStore();

  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const name = e.target.value as SampleProgramName;
    setSelectedSample(name);
    loadSample(name);
  };

  return (
    <div className="sample-selector">
      <label htmlFor="sample-select">Examples:</label>
      <select
        id="sample-select"
        value={selectedSample}
        onChange={handleChange}
        className="sample-select"
      >
        {Object.entries(SAMPLE_PROGRAM_LABELS).map(([key, label]) => (
          <option key={key} value={key}>
            {label}
          </option>
        ))}
      </select>
    </div>
  );
}

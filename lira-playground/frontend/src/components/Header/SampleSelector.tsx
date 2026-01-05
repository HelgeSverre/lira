import { useEditorStore } from '../../stores/editorStore';
import { useUiStore } from '../../stores/uiStore';
import { getSamplesByCategory } from '../../samples';
import './SampleSelector.css';

export function SampleSelector() {
  const { loadSample } = useEditorStore();
  const { selectedSample, setSelectedSample } = useUiStore();

  const samplesByCategory = getSamplesByCategory();

  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const id = e.target.value;
    setSelectedSample(id);
    loadSample(id);
  };

  return (
    <div className="sample-selector" data-testid="sample-selector">
      <label htmlFor="sample-select">Examples:</label>
      <select
        id="sample-select"
        value={selectedSample}
        onChange={handleChange}
        className="sample-select"
        data-testid="sample-select"
      >
        {Array.from(samplesByCategory).map(([category, samples]) => (
          <optgroup key={category} label={category}>
            {samples.map((sample) => (
              <option key={sample.metadata.id} value={sample.metadata.id}>
                {sample.metadata.title}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
    </div>
  );
}

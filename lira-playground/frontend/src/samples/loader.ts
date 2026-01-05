import type { Sample, SampleMetadata } from './types';

/**
 * Parse metadata from a sample file's header comments
 */
function parseMetadata(filename: string, source: string): SampleMetadata {
  const lines = source.split('\n');
  const id = filename.replace(/\.li$/, '');

  let title: string | undefined;
  let description: string | undefined;
  let order: number | undefined;
  let category: string | undefined;

  for (const line of lines) {
    // Stop parsing when we hit non-comment line
    if (!line.startsWith('//')) break;

    const titleMatch = line.match(/^\/\/\s*@title:\s*(.+)$/);
    if (titleMatch) title = titleMatch[1].trim();

    const descMatch = line.match(/^\/\/\s*@description:\s*(.+)$/);
    if (descMatch) description = descMatch[1].trim();

    const orderMatch = line.match(/^\/\/\s*@order:\s*(\d+)$/);
    if (orderMatch) order = parseInt(orderMatch[1], 10);

    const categoryMatch = line.match(/^\/\/\s*@category:\s*(.+)$/);
    if (categoryMatch) category = categoryMatch[1].trim();
  }

  // Validation
  if (!title) {
    throw new Error(`Sample ${filename} missing @title directive`);
  }
  if (!description) {
    throw new Error(`Sample ${filename} missing @description directive`);
  }
  if (order === undefined) {
    throw new Error(`Sample ${filename} missing @order directive`);
  }

  return { id, title, description, order, category };
}

/**
 * Load all samples using Vite's glob imports
 */
function loadSamples(): { samples: Map<string, Sample>; orderedIds: string[] } {
  // Import all .li files from the samples directory as raw strings
  const modules = import.meta.glob('/samples/*.li', {
    eager: true,
    query: '?raw',
    import: 'default',
  }) as Record<string, string>;

  const samples = new Map<string, Sample>();
  const sampleList: Sample[] = [];

  for (const [path, source] of Object.entries(modules)) {
    // Extract filename from path (e.g., "/samples/hello-world.li" -> "hello-world.li")
    const filename = path.split('/').pop()!;

    const metadata = parseMetadata(filename, source);
    const sample: Sample = { metadata, source };

    samples.set(metadata.id, sample);
    sampleList.push(sample);
  }

  // Sort by order
  sampleList.sort((a, b) => a.metadata.order - b.metadata.order);
  const orderedIds = sampleList.map(s => s.metadata.id);

  return { samples, orderedIds };
}

// Load samples once at module initialization
const { samples, orderedIds } = loadSamples();

/**
 * Get a sample by its ID
 */
export function getSample(id: string): Sample | undefined {
  return samples.get(id);
}

/**
 * Get the source code of a sample by its ID
 */
export function getSampleSource(id: string): string {
  return samples.get(id)?.source ?? '';
}

/**
 * Get all samples in display order
 */
export function getOrderedSamples(): Sample[] {
  return orderedIds.map(id => samples.get(id)!);
}

/**
 * Get the ID of the default (first) sample
 */
export function getDefaultSampleId(): string {
  return orderedIds[0] ?? '';
}

/**
 * Get all sample IDs in display order
 */
export function getOrderedSampleIds(): string[] {
  return orderedIds;
}

/** Category display order */
const CATEGORY_ORDER = ['Getting Started', 'Fundamentals', 'Advanced'];

/**
 * Get samples grouped by category, maintaining order within each category
 */
export function getSamplesByCategory(): Map<string, Sample[]> {
  const categoryMap = new Map<string, Sample[]>();

  // Initialize categories in desired order
  for (const cat of CATEGORY_ORDER) {
    categoryMap.set(cat, []);
  }

  // Group samples by category
  for (const id of orderedIds) {
    const sample = samples.get(id)!;
    const category = sample.metadata.category ?? 'Uncategorized';

    if (!categoryMap.has(category)) {
      categoryMap.set(category, []);
    }
    categoryMap.get(category)!.push(sample);
  }

  // Remove empty categories
  for (const [cat, items] of categoryMap) {
    if (items.length === 0) {
      categoryMap.delete(cat);
    }
  }

  return categoryMap;
}

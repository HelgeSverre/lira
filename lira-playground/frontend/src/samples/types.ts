/**
 * Metadata extracted from sample file headers
 */
export interface SampleMetadata {
  /** Unique identifier derived from filename (e.g., "hello-world") */
  id: string;
  /** Display title from @title directive */
  title: string;
  /** Description from @description directive */
  description: string;
  /** Sort order from @order directive */
  order: number;
  /** Category for grouping from @category directive */
  category?: string;
}

/**
 * A loaded sample program with metadata and source code
 */
export interface Sample {
  metadata: SampleMetadata;
  /** The full source code (including header comments) */
  source: string;
}

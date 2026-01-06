#!/usr/bin/env node

import { execSync } from 'child_process';
import { readdirSync } from 'fs';
import { join, dirname, resolve } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const samplesDir = join(__dirname, '..', 'samples');

// Get the project root (where Cargo.toml is)
const projectRoot = resolve(__dirname, '..', '..', '..');

const files = readdirSync(samplesDir).filter(f => f.endsWith('.li'));

console.log('Verifying sample programs...');
console.log(`Found ${files.length} samples in ${samplesDir}\n`);

let failed = false;
const errors = [];

for (const file of files) {
  const filePath = join(samplesDir, file);
  try {
    // Use cargo run to ensure we use the latest compiler from the repo
    execSync(`cargo run --package lirac --bin lirac -- check "${filePath}"`, {
      stdio: 'pipe',
      cwd: projectRoot,
    });
    console.log(`  ✓ ${file}`);
  } catch (error) {
    console.error(`  ✗ ${file}`);
    const stderr = error.stderr?.toString() || error.message;
    errors.push({ file, error: stderr });
    failed = true;
  }
}

console.log('');

if (failed) {
  console.error('Sample verification failed!\n');
  for (const { file, error } of errors) {
    console.error(`--- ${file} ---`);
    console.error(error);
    console.error('');
  }
  process.exit(1);
}

console.log(`All ${files.length} samples verified successfully!`);

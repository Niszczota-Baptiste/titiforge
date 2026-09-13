// Fabrique r.0.1.mca avec le générateur de bench de we-engine, puis le pose sur
// disque. Le MÊME fichier servira au bench JS et au prototype Rust.
import { writeFileSync } from 'node:fs';
import { buildTerrainRegion } from '../../../ExeWorldEdit/packages/we-engine/bench/lib/fixtures.js';

const t0 = performance.now();
const buf = buildTerrainRegion({ chunks: 32, sections: 24, seed: 7 });
const ms = performance.now() - t0;
const out = './r.0.1.mca';
writeFileSync(out, buf);
console.log(`région pleine écrite en ${ms.toFixed(0)} ms`);
console.log(`  ${out}`);
console.log(`  ${(buf.length / 1048576).toFixed(2)} Mio — 1024 chunks × 24 sections = 24576 sections = ${(1024*24*4096).toLocaleString('fr')} blocs`);
